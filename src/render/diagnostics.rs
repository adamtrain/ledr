/* Copyright © 2024-2026 Adam Train <adam@adametrain.com>
 *
 * This program is free software: you can redistribute it and/or modify
 * it under the terms of the GNU General Public License as published by
 * the Free Software Foundation, either version 3 of the License, or
 * (at your option) any later version.
 *
 * This program is distributed in the hope that it will be useful,
 * but WITHOUT ANY WARRANTY; without even the implied warranty of
 * MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the
 * GNU General Public License for more details.
 *
 * You should have received a copy of the GNU General Public License
 * along with this program. If not, see <https://www.gnu.org/licenses/>.
 */

//! Errors and warnings, shown with the ledger lines they are about.

use crate::diagnostics::{Diagnostic, Severity};
use crate::parsing::loader::Diagnostics;
use crate::render::{Mode, plural};
use crate::syntax::source::SourceMap;
use crate::ui::style::{Rgb, Style, palette};
use crate::ui::text::{Doc, Line, width};

/// Entries longer than this are cut short in snippets
const MAX_SNIPPET_LINES: usize = 8;

fn look(severity: Severity) -> (&'static str, &'static str, Rgb) {
	match severity {
		Severity::Error => ("✗", "error", palette::RED),
		Severity::Warning => ("⚠", "warning", palette::AMBER),
	}
}

/// Any error, shown as well as possible: diagnostics with their source
/// lines, several of them in turn, and anything else as a plain message.
pub fn error(error: &anyhow::Error, sources: &SourceMap, mode: Mode) -> String {
	let diagnostics: Vec<Diagnostic> =
		if let Some(all) = error.downcast_ref::<Diagnostics>() {
			all.0.clone()
		} else if let Some(one) = error.downcast_ref::<Diagnostic>() {
			vec![one.clone()]
		} else {
			vec![Diagnostic::error(capitalize(&format!("{error:#}")))]
		};

	let mut out = String::new();
	for (i, d) in diagnostics.iter().enumerate() {
		match mode {
			Mode::Plain => out.push_str(&plain(d, sources)),
			Mode::Fancy { .. } => {
				if i > 0 {
					out.push('\n');
				}
				out.push_str(&mode.render(&fancy_width(
					d,
					sources,
					mode.width(),
				)));
			},
		}
	}
	if diagnostics.len() > 1 && mode.is_fancy() {
		let summary = Line::styled("\n✗ ", Style::color(palette::RED).bold())
			.with(plural(diagnostics.len(), "error"), Style::new().bold());
		out.push_str(&summary.render(mode.color()));
		out.push('\n');
	}
	out
}

fn capitalize(s: &str) -> String {
	let mut chars = s.chars();
	match chars.next() {
		Some(first) => first.to_uppercase().chain(chars).collect(),
		None => String::new(),
	}
}

/// One line per diagnostic plus its help, the way compilers report, so
/// editors and scripts can jump to `path:line`.
pub fn plain(diagnostic: &Diagnostic, sources: &SourceMap) -> String {
	let (_, word, _) = look(diagnostic.severity);
	let mut out = match &diagnostic.span {
		Some(span) => format!(
			"{word}: {}: {}\n",
			sources.describe(span),
			diagnostic.message
		),
		None => format!("{word}: {}\n", diagnostic.message),
	};
	for note in &diagnostic.notes {
		out.push_str(&format!("  note: {note}\n"));
	}
	if let Some(help) = &diagnostic.help {
		out.push_str(&format!("  help: {help}\n"));
	}
	out
}

/// The diagnostic with its source lines, a pointer at the problem, and any
/// notes and help, in the style of a friendly compiler.
pub fn fancy(diagnostic: &Diagnostic, sources: &SourceMap) -> Doc {
	fancy_width(diagnostic, sources, crate::ui::style::terminal_width())
}

/// Splits text into lines of at most `width` columns, at spaces
fn wrap(text: &str, width: usize) -> Vec<String> {
	let mut lines = vec![];
	let mut line = String::new();
	for word in text.split(' ') {
		if !line.is_empty()
			&& crate::ui::text::width(&line) + 1 + crate::ui::text::width(word)
				> width
		{
			lines.push(std::mem::take(&mut line));
		}
		if !line.is_empty() {
			line.push(' ');
		}
		line.push_str(word);
	}
	lines.push(line);
	lines
}

/// [`fancy`], laid out to fit a given width
pub fn fancy_width(
	diagnostic: &Diagnostic,
	sources: &SourceMap,
	total_width: usize,
) -> Doc {
	let (glyph, _, color) = look(diagnostic.severity);
	let faint = Style::faint();
	let mut doc = Doc::new();

	doc.push(
		Line::styled(format!("{glyph} "), Style::color(color).bold())
			.with(diagnostic.message.clone(), Style::new().bold()),
	);

	let gutter = diagnostic
		.span
		.map_or(1, |s| s.last_line.to_string().len())
		.max(2);
	let blank = " ".repeat(gutter);

	let mut tail: Vec<(String, String, Style)> = vec![];
	for note in &diagnostic.notes {
		tail.push(("note".into(), note.clone(), faint));
	}
	if let Some(help) = &diagnostic.help {
		tail.push(("help".into(), help.clone(), Style::color(palette::ACCENT)));
	}

	let Some(span) = diagnostic.span else {
		for (kind, text, style) in tail {
			doc.push(
				Line::styled(format!("  {kind}: "), style)
					.with(text, Style::new()),
			);
		}
		return doc;
	};

	let file = sources.get(span.file);
	doc.push(
		Line::faint(format!("{blank} ╭─ "))
			.with(sources.describe(&span), Style::color(palette::ACCENT)),
	);

	let last = span.last_line.min(span.line + MAX_SNIPPET_LINES - 1);
	for number in span.line..=last {
		let text = file.line(number).replace('\t', "    ");
		doc.push(
			Line::faint(format!("{number:>gutter$} │ "))
				.with(text, Style::new()),
		);
	}
	if last < span.last_line {
		doc.push(Line::faint(format!(
			"{blank} │ … {} more lines",
			span.last_line - last
		)));
	}

	// Point at the exact columns, if the span is part of one line
	let pointed = !span.is_multiline() && span.end > span.start;
	if pointed {
		let raw = file.line(span.line);
		let before = raw.get(..span.start).unwrap_or(raw).replace('\t', "    ");
		let target = raw
			.get(span.start..span.end)
			.unwrap_or("")
			.replace('\t', "    ");
		let mut caret = Line::faint(format!("{blank} │ "))
			.with(" ".repeat(width(&before)), Style::new())
			.with(
				"^".repeat(width(&target).max(1)),
				Style::color(color).bold(),
			);
		if let Some(label) = &diagnostic.label {
			caret.push(format!(" {label}"), Style::color(color));
		}
		doc.push(caret);
	} else if let Some(label) = &diagnostic.label {
		doc.push(
			Line::faint(format!("{blank} │ "))
				.with(label.clone(), Style::color(color)),
		);
	}

	let count = tail.len();
	for (i, (kind, text, style)) in tail.into_iter().enumerate() {
		let last = i + 1 == count;
		let corner = if last { "╰─" } else { "├─" };
		let indent = width(&blank) + 4 + kind.len() + 2;
		let lines = wrap(&text, total_width.saturating_sub(indent).max(20));
		for (j, line) in lines.into_iter().enumerate() {
			doc.push(if j == 0 {
				Line::faint(format!("{blank} {corner} "))
					.with(format!("{kind}: "), style)
					.with(line, Style::new())
			} else {
				let rail = if last { " " } else { "│" };
				Line::faint(format!(
					"{blank} {rail}  {}",
					" ".repeat(kind.len() + 2)
				))
				.with(line, Style::new())
			});
		}
	}
	if count == 0 {
		doc.push(Line::faint(format!("{blank} ╰─")));
	}
	doc
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::syntax::source::{SourceFile, Span};

	fn sources() -> (SourceMap, crate::syntax::source::FileId) {
		let mut map = SourceMap::new();
		let id = map.add(SourceFile::new(
			"money.ledr".into(),
			"2024-01-01 Coffee\n    Assets:Cash   100\n".into(),
		));
		(map, id)
	}

	#[test]
	fn test_plain() {
		let (map, file) = sources();
		let d = Diagnostic::error("Missing a currency")
			.at(Span::new(file, 2, 18, 19))
			.help("write the currency");
		assert_eq!(
			plain(&d, &map),
			"error: money.ledr:2: Missing a currency\n  help: write the currency\n"
		);
	}

	#[test]
	fn test_fancy_points_at_the_problem() {
		let (map, file) = sources();
		let d = Diagnostic::error("Missing a currency")
			.at(Span::new(file, 2, 14, 17))
			.label("this amount")
			.help("write the currency");
		let text = fancy_width(&d, &map, 80)
			.render(crate::ui::style::ColorLevel::None);
		assert_eq!(
			text,
			"✗ Missing a currency\n   ╭─ money.ledr:2\n 2 │     Assets:Cash   100\n   │               ^^^ this amount\n   ╰─ help: write the currency\n"
		);
	}

	#[test]
	fn test_long_help_wraps_under_itself() {
		let (map, file) = sources();
		let d = Diagnostic::error("x")
			.at(Span::new(file, 2, 4, 15))
			.help("one two three four five six seven eight nine ten");
		let text = fancy_width(&d, &map, 30)
			.render(crate::ui::style::ColorLevel::None);
		assert!(text.ends_with("   ╰─ help: one two three four\n            five six seven eight\n            nine ten\n"), "{text}");
	}

	#[test]
	fn test_fancy_shows_whole_entries() {
		let (map, file) = sources();
		let entry = Span::new(file, 1, 0, 0).through(Span::new(file, 2, 0, 0));
		let d = Diagnostic::warning("Looks odd").at(entry);
		let text = fancy(&d, &map).render(crate::ui::style::ColorLevel::None);
		assert!(
			text.contains(
				" 1 │ 2024-01-01 Coffee\n 2 │     Assets:Cash   100\n"
			)
		);
	}
}
