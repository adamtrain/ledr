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

//! A formatter for ledger files that changes how they look, never what they
//! mean: amounts line up on their decimal points, spacing and dates are made
//! consistent, and every comment stays where it was.

use crate::diagnostics::Diagnostic;
use crate::syntax::lexer::{Line, LineKind, Posting, lex_line};
use crate::syntax::source::FileId;
use crate::ui::style::{Style, palette};
use crate::ui::text::{Doc, Line as Styled};
use anyhow::{Error, bail};
use similar::{ChangeTag, TextDiff};
use std::path::Path;

/// Account names longer than this don't push every amount in the file right
const MAX_ACCOUNT_COLUMN: usize = 48;

/// Runs of more blank lines than this are shortened
const MAX_BLANK_LINES: usize = 2;

/// How a file lays out its postings, so new text can match it
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileStyle {
	/// What postings are indented with: a tab, or some spaces
	pub indent: String,
	/// Width reserved for account names, before the amounts
	pub account_width: usize,
	/// Width reserved for the whole-number part of amounts, so that decimal
	/// points line up
	pub whole_width: usize,
}

impl Default for FileStyle {
	fn default() -> Self {
		Self {
			indent: "    ".into(),
			account_width: 0,
			whole_width: 0,
		}
	}
}

/// Splits an amount as written into its whole part and the rest, e.g.
/// `-1,234.50` into `-1,234` and `.50`
fn split_amount(text: &str) -> (&str, &str) {
	match text.find('.') {
		Some(i) => (&text[..i], &text[i..]),
		None => (text, ""),
	}
}

impl FileStyle {
	/// Learns a file's indentation from its postings, and sizes the columns
	/// to fit all of them
	pub fn detect<'a>(
		lines: impl IntoIterator<Item = (&'a str, &'a Line)>,
	) -> Self {
		let mut tabs = 0;
		let mut spaces: Vec<usize> = vec![];
		let mut style = Self::default();

		for (raw, line) in lines {
			let LineKind::Posting(posting) = &line.kind else {
				continue;
			};
			if raw.starts_with('\t') {
				tabs += 1;
			} else {
				spaces.push(raw.len() - raw.trim_start_matches(' ').len());
			}
			style.fit(posting);
		}

		if tabs > spaces.len() {
			style.indent = "\t".into();
		} else if let Some(common) = most_common(&spaces).filter(|&n| n > 0) {
			style.indent = " ".repeat(common);
		}
		style
	}

	/// Widens the columns to fit a posting
	pub fn fit(&mut self, posting: &Posting) {
		if let Some(amount) = &posting.amount {
			let account = posting.account.chars().count();
			if account <= MAX_ACCOUNT_COLUMN {
				self.account_width = self.account_width.max(account);
			}
			let (whole, _) = split_amount(&amount.value_text);
			self.whole_width = self.whole_width.max(whole.chars().count());
		}
	}

	/// A posting laid out in this style, without its indentation or comment
	pub fn posting(&self, posting: &Posting) -> String {
		let Some(amount) = &posting.amount else {
			return posting.account.clone();
		};
		let account_len = posting.account.chars().count();
		let (whole, fraction) = split_amount(&amount.value_text);
		let gap = self.account_width.saturating_sub(account_len) + 2;
		let pad = self.whole_width.saturating_sub(whole.chars().count());

		let mut out = format!(
			"{}{}{whole}{fraction} {}",
			posting.account,
			" ".repeat(gap + pad),
			amount.currency
		);
		if let Some(price) = &amount.price {
			let op = if price.is_total { "@@" } else { "@" };
			out.push_str(&format!(
				" {op} {} {}",
				price.value_text, price.currency
			));
		}
		if let Some(lot) = &amount.lot {
			match &lot.name {
				Some(name) => out.push_str(&format!(
					" {{ {} {}, \"{name}\" }}",
					lot.cost_text, lot.currency
				)),
				None => out.push_str(&format!(
					" {{ {} {} }}",
					lot.cost_text, lot.currency
				)),
			}
		}
		out
	}
}

fn most_common(values: &[usize]) -> Option<usize> {
	let mut counts = std::collections::BTreeMap::new();
	for v in values {
		*counts.entry(*v).or_insert(0) += 1;
	}
	counts
		.into_iter()
		.max_by_key(|(v, n)| (*n, *v))
		.map(|(v, _)| v)
}

fn with_comment(content: String, comment: &Option<String>) -> String {
	match comment {
		Some(c) if content.trim().is_empty() => format!("{content}{c}"),
		Some(c) => format!("{content}  {c}"),
		None => content,
	}
}

/// Formats one file's text. Fails on a syntax error, and refuses to return
/// anything whose meaning differs from the original.
pub fn tidy(text: &str, file: FileId) -> Result<String, Error> {
	let raw_lines: Vec<&str> = text.lines().collect();
	let mut lexed = Vec::with_capacity(raw_lines.len());
	for (i, raw) in raw_lines.iter().enumerate() {
		lexed.push(lex_line(raw, file, i + 1)?);
	}
	let style = FileStyle::detect(raw_lines.iter().copied().zip(&lexed));

	let mut out: Vec<String> = vec![];
	let mut in_entry = false;
	let mut blank_run = 0;
	for line in &lexed {
		if line.kind == LineKind::Blank {
			in_entry = false;
			blank_run += 1;
			if blank_run <= MAX_BLANK_LINES && !out.is_empty() {
				out.push(String::new());
			}
			continue;
		}
		blank_run = 0;

		let indent = &style.indent;
		let content = match &line.kind {
			LineKind::Blank => unreachable!(),
			LineKind::CommentOnly => {
				if in_entry {
					indent.clone()
				} else {
					String::new()
				}
			},
			LineKind::Include(include) => {
				if include.path.contains(char::is_whitespace) {
					format!("include \"{}\"", include.path)
				} else {
					format!("include {}", include.path)
				}
			},
			LineKind::Directive(d) => format!(
				"! {} {} {}",
				d.date,
				d.directive.keyword(),
				d.directive.args()
			),
			LineKind::Header(header) => {
				in_entry = true;
				format!("{} {}", header.date, header.description)
			},
			LineKind::Reference(text) if text.is_empty() => {
				format!("{indent}//")
			},
			LineKind::Reference(text) => format!("{indent}// {text}"),
			LineKind::Posting(posting) => {
				format!("{indent}{}", style.posting(posting))
			},
		};
		out.push(with_comment(content, &line.comment));
	}

	while out.last().is_some_and(String::is_empty) {
		out.pop();
	}
	// Keep Windows line endings in a file that uses them
	let newline = if text.contains("\r\n") { "\r\n" } else { "\n" };
	let mut tidied = out.join(newline);
	if !tidied.is_empty() {
		tidied.push_str(newline);
	}

	// The safety net: the result must mean exactly what the original did
	let before = meaning(&lexed);
	let after: Vec<Line> = tidied
		.lines()
		.enumerate()
		.map(|(i, raw)| lex_line(raw, file, i + 1))
		.collect::<Result<_, Diagnostic>>()?;
	if before != meaning(&after) {
		bail!(
			"tidying would change what this file means; leaving it alone (this is a bug in ledr)"
		);
	}
	Ok(tidied)
}

/// A file's lines with everything that only affects layout removed: column
/// positions, and how many blank lines separate things.
fn meaning(lines: &[Line]) -> Vec<Line> {
	let mut out: Vec<Line> = vec![];
	for line in lines {
		let mut line = line.clone();
		strip_columns(&mut line.kind);
		let repeat_blank = line.kind == LineKind::Blank
			&& out.last().is_none_or(|l| l.kind == LineKind::Blank);
		if !repeat_blank {
			out.push(line);
		}
	}
	while out.last().is_some_and(|l| l.kind == LineKind::Blank) {
		out.pop();
	}
	out
}

fn strip_columns(kind: &mut LineKind) {
	use crate::syntax::lexer::Cols;
	let zero = Cols { start: 0, end: 0 };
	match kind {
		LineKind::Include(i) => i.cols = zero,
		LineKind::Directive(d) => d.args = zero,
		LineKind::Posting(p) => {
			p.account_cols = zero;
			if let Some(a) = &mut p.amount {
				a.cols = zero;
				if let Some(price) = &mut a.price {
					price.cols = zero;
				}
				if let Some(lot) = &mut a.lot {
					lot.cols = zero;
				}
			}
		},
		_ => {},
	}
}

/// A unified diff between two versions of a file, as `patch` expects
pub fn plain_diff(path: &Path, before: &str, after: &str) -> String {
	let name = path.display().to_string();
	TextDiff::from_lines(before, after)
		.unified_diff()
		.context_radius(2)
		.header(&format!("a/{name}"), &format!("b/{name}"))
		.to_string()
}

/// A colored diff, with line numbers from the original file
pub fn fancy_diff(path: &Path, before: &str, after: &str) -> Doc {
	let diff = TextDiff::from_lines(before, after);
	let mut doc = Doc::new();
	doc.push(Styled::styled("◇ ", Style::color(palette::ACCENT)).with(
		path.display().to_string(),
		Style::color(palette::ACCENT).bold(),
	));
	for (i, group) in diff.grouped_ops(2).iter().enumerate() {
		if i > 0 {
			doc.push(Styled::faint("   ┈"));
		}
		for op in group {
			for change in diff.iter_changes(op) {
				let text =
					change.value().trim_end_matches('\n').replace('\t', "    ");
				let number = change
					.old_index()
					.map_or(String::new(), |n| (n + 1).to_string());
				let gutter = Styled::faint(format!("{number:>5} "));
				doc.push(match change.tag() {
					ChangeTag::Delete => gutter
						.with("- ", Style::color(palette::RED))
						.with(text, Style::color(palette::RED)),
					ChangeTag::Insert => Styled::faint("      ")
						.with("+ ", Style::color(palette::GREEN))
						.with(text, Style::color(palette::GREEN)),
					ChangeTag::Equal => gutter
						.with("  ", Style::new())
						.with(text, Style::faint()),
				});
			}
		}
	}
	doc
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::syntax::source::{SourceFile, SourceMap};

	fn file() -> FileId {
		SourceMap::new().add(SourceFile::new("t".into(), String::new()))
	}

	fn run(text: &str) -> String {
		tidy(text, file()).unwrap()
	}

	#[test]
	fn test_aligns_on_decimal_points() {
		let messy = "\
2024-1-5 Groceries, and more   # weekly
  Expenses:Food    42.1 USD
  Assets:Checking:Main   -1,042.10  USD
  Equity:Rounding
";
		assert_eq!(
			run(messy),
			"\
2024-01-05 Groceries, and more  # weekly
  Expenses:Food             42.1 USD
  Assets:Checking:Main  -1,042.10 USD
  Equity:Rounding
"
		);
	}

	#[test]
	fn test_keeps_comments_and_structure() {
		let text = "\
# Header comment


! 2024-01-01   account    Assets:Cash # declared



2024-01-02 Thing
\t// ref
\t# inside
\tAssets:Cash\t10 USD @ 1.3   CAD
\tAssets:Cash\t-2 AAPL {5 USD,\"lot A\"}
\tEquity:X
";
		assert_eq!(
			run(text),
			"\
# Header comment


! 2024-01-01 account Assets:Cash  # declared


2024-01-02 Thing
\t// ref
\t# inside
\tAssets:Cash  10 USD @ 1.3 CAD
\tAssets:Cash  -2 AAPL { 5 USD, \"lot A\" }
\tEquity:X
"
		);
	}

	#[test]
	fn test_keeps_windows_line_endings() {
		let text = "2024-1-2 X\r\n  Assets:A  1 USD\r\n  Assets:B\r\n";
		let tidied = run(text);
		assert_eq!(
			tidied,
			"2024-01-02 X\r\n  Assets:A  1 USD\r\n  Assets:B\r\n"
		);
		assert_eq!(run(&tidied), tidied);
	}

	#[test]
	fn test_is_idempotent() {
		let text =
			"2024-01-02 X\n    Assets:A  1 USD\n    Assets:Bbbbb  -1.00 USD\n";
		let once = run(text);
		assert_eq!(run(&once), once);
	}

	#[test]
	fn test_refuses_files_with_syntax_errors() {
		assert!(tidy("2024-13-01 Nope\n", file()).is_err());
	}

	#[test]
	fn test_detects_indentation() {
		let text = "2024-01-02 X\n  Assets:A  1 USD\n  Assets:B\n";
		let lines: Vec<Line> = text
			.lines()
			.enumerate()
			.map(|(i, l)| lex_line(l, file(), i + 1).unwrap())
			.collect();
		let style = FileStyle::detect(text.lines().zip(&lines));
		assert_eq!(style.indent, "  ");
		assert_eq!(style.account_width, 8);
		assert_eq!(style.whole_width, 1);
	}
}
