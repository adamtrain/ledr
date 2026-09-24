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

//! Entries printed as ledger text, with syntax highlighting.

use crate::gl::entry::Entry;
use crate::render::{account, plural};
use crate::ui::style::{Style, palette};
use crate::ui::text::{Doc, Line, width};
use crate::ui::widgets::Decimals;

/// Whether a search term matches a description: case-insensitively, unless
/// the term has capitals in it ("smart case", as in ripgrep).
pub fn matches(description: &str, term: &str) -> bool {
	find(description, term).is_some()
}

fn find(haystack: &str, term: &str) -> Option<(usize, usize)> {
	if term.is_empty() {
		return None;
	}
	if term.chars().any(char::is_uppercase) {
		return haystack.find(term).map(|i| (i, i + term.len()));
	}
	// Lowercasing can change byte lengths, so search char by char
	let lower: Vec<char> = term.chars().collect();
	let chars: Vec<(usize, char)> = haystack.char_indices().collect();
	(0..chars.len()).find_map(|start| {
		let mut end = start;
		for &t in &lower {
			let (_, c) = *chars.get(end)?;
			if !c.to_lowercase().eq(t.to_lowercase()) {
				return None;
			}
			end += 1;
		}
		let byte_end = chars.get(end).map_or(haystack.len(), |(i, _)| *i);
		Some((chars[start].0, byte_end))
	})
}

/// Every entry, highlighted, with the search term marked if there is one
pub fn entries(
	entries: &[&Entry],
	term: Option<&str>,
	heading: Option<Line>,
) -> Doc {
	let mut doc = Doc::new();
	if let Some(heading) = heading {
		doc.push(heading);
		doc.blank();
	}

	for entry in entries {
		doc.extend(entry_lines(entry, term));
		doc.blank();
	}
	doc
}

/// A heading for search results
pub fn search_heading(count: usize, term: &str) -> Line {
	Line::styled("◇ ", Style::color(palette::ACCENT))
		.with(plural(count, "entry"), Style::color(palette::ACCENT).bold())
		.with(format!(" · matching “{term}”"), Style::faint())
}

fn entry_lines(entry: &Entry, term: Option<&str>) -> Vec<Line> {
	let mut lines = vec![];

	let mut header = Line::styled(
		entry.get_date().to_string(),
		Style::color(palette::ACCENT),
	)
	.with(" ", Style::new());
	let desc = entry.get_desc();
	let bold = Style::new().bold();
	match term.and_then(|t| find(desc, t)) {
		Some((start, end)) => {
			header.push(&desc[..start], bold);
			header.push(
				&desc[start..end],
				Style::color(palette::INK).on(palette::AMBER).bold(),
			);
			header.push(&desc[end..], bold);
		},
		None => header.push(desc.clone(), bold),
	}
	lines.push(header);

	let reference = entry.get_reference();
	for line in reference.lines() {
		lines.push(
			Line::faint(format!("    // {line}"))
				.with("", Style::new().italic()),
		);
	}

	let details = entry.details();
	let account_width = details
		.iter()
		.map(|d| width(d.account()))
		.max()
		.unwrap_or(0);
	let numbers: Vec<String> =
		details.iter().map(|d| d.value().to_string()).collect();
	let decimals = Decimals::fit(numbers.iter().map(String::as_str));

	for (detail, number) in details.iter().zip(&numbers) {
		// The line left blank to balance the entry keeps its account's
		// color; only the amount ledr worked out for it is faint. Lines
		// ledr added itself, like conversions, are faint altogether.
		let balancing = detail.is_system()
			&& entry.virtual_detail() == Some(detail.account().as_str());
		let name = if detail.is_system() && !balancing {
			Line::faint(detail.account().clone())
		} else {
			account(detail.account())
		};
		let pad = " ".repeat(account_width - width(detail.account()));
		let number_style = if detail.is_system() {
			Style::faint()
		} else if detail.value().is_negative() {
			Style::color(palette::RED)
		} else {
			Style::new()
		};
		lines.push(
			Line::plain("    ")
				.then(name)
				.with(pad, Style::new())
				.with("  ", Style::new())
				.with(decimals.align(number), number_style)
				.with(format!(" {}", detail.currency()), Style::faint()),
		);
	}
	lines
}

/// Ledger text colored by what each part is, keeping its exact spacing
pub fn highlight(text: &str) -> Vec<Line> {
	let mut map = crate::syntax::source::SourceMap::new();
	let file = map.add(crate::syntax::source::SourceFile::new(
		"x".into(),
		String::new(),
	));
	text.lines()
		.enumerate()
		.map(|(i, raw)| {
			match crate::syntax::lexer::lex_line(raw, file, i + 1) {
				Ok(line) => highlight_line(raw, &line),
				Err(_) => Line::plain(raw),
			}
		})
		.collect()
}

fn highlight_line(raw: &str, line: &crate::syntax::lexer::Line) -> Line {
	use crate::syntax::lexer::LineKind;
	let comment_at = raw.find('#').unwrap_or(raw.len());
	let (content, comment) = raw.split_at(comment_at);
	let mut out = match &line.kind {
		LineKind::Header(_) => {
			let date_end =
				content.find(char::is_whitespace).unwrap_or(content.len());
			Line::styled(&content[..date_end], Style::color(palette::ACCENT))
				.with(&content[date_end..], Style::new().bold())
		},
		LineKind::Directive(_) => {
			Line::styled(content.to_string(), Style::color(palette::PURPLE))
		},
		LineKind::Posting(p) => {
			let a = p.account_cols;
			let mut l = Line::plain(&content[..a.start])
				.then(account(&content[a.start..a.end]));
			match &p.amount {
				Some(amount) => {
					let c = amount.cols;
					let value_end = c.start + amount.value_text.len();
					let style = if amount.value.is_negative() {
						Style::color(palette::RED)
					} else {
						Style::new().bold()
					};
					l.push(&content[a.end..c.start], Style::new());
					l.push(&content[c.start..value_end], style);
					l.push(&content[value_end..], Style::faint());
				},
				None => l.push(&content[a.end..], Style::new()),
			}
			l
		},
		LineKind::Reference(_) | LineKind::CommentOnly => {
			Line::styled(content.to_string(), Style::faint())
		},
		_ => Line::plain(content),
	};
	out.push(comment, Style::faint().italic());
	out
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn test_highlight_keeps_text() {
		let text = "! 2024-01-01 account Assets:A\n2024-01-02 Thing  # hi\n    Assets:A   -1.00 USD\n    Expenses:B\n";
		let lines = highlight(text);
		let back: Vec<String> = lines.iter().map(|l| l.text()).collect();
		assert_eq!(back.join("\n") + "\n", text);
	}

	#[test]
	fn test_smart_case() {
		assert!(matches("Tim Hortons", "tim"));
		assert!(matches("Tim Hortons", "HORTONS".to_lowercase().as_str()));
		assert!(matches("Tim Hortons", "Tim"));
		assert!(!matches("Tim Hortons", "TIM"));
		assert!(!matches("tim hortons", "Tim"));
		assert_eq!(find("Café Crème", "crème"), Some((6, 12)));
	}
}
