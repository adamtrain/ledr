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

//! Putting a new entry together, checking it in context, and saving it.

use crate::diagnostics::Diagnostic;
use crate::input::history::History;
use crate::parsing::loader::Overlay;
use crate::parsing::{LoadOptions, Loaded, load_ledger};
use crate::syntax::lexer::{
	Cols, Line, LineKind, Posting, PostingAmount, lex_line,
};
use crate::syntax::source::{FileId, SourceMap};
use crate::tidy::FileStyle;
use crate::util::amount::DEFAULT_PRECISION;
use crate::util::date::Date;
use crate::util::quant::Quant;
use anyhow::{Error, anyhow, bail};
use std::collections::BTreeMap;
use std::io::Write;
use std::path::{Path, PathBuf};

/// An entry being written
#[derive(Clone, Debug, PartialEq)]
pub struct Draft {
	pub date: Date,
	pub description: String,
	pub postings: Vec<DraftPosting>,
	/// How the draft was put together, to show alongside it
	pub notes: Vec<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct DraftPosting {
	pub account: String,
	/// None for the one line that balances the others
	pub amount: Option<(Quant, String)>,
}

impl DraftPosting {
	pub fn new(account: &str, amount: Option<(Quant, &str)>) -> Self {
		Self {
			account: account.to_string(),
			amount: amount.map(|(v, c)| (v, c.to_string())),
		}
	}
}

impl Draft {
	/// The sum of the amounts written, by currency, leaving out zeros
	pub fn imbalance(&self) -> BTreeMap<String, Quant> {
		let mut sums: BTreeMap<String, Quant> = BTreeMap::new();
		for (value, currency) in
			self.postings.iter().filter_map(|p| p.amount.as_ref())
		{
			*sums.entry(currency.clone()).or_default() += *value;
		}
		sums.retain(|_, v| !v.is_zero());
		sums
	}

	pub fn has_blank(&self) -> bool {
		self.postings.iter().any(|p| p.amount.is_none())
	}

	/// Whether the entry is finished: it balances, or has a line to balance
	/// it, or converts between exactly two currencies
	pub fn is_balanced(&self) -> bool {
		self.postings.len() >= 2
			&& (self.has_blank()
				|| self.imbalance().is_empty()
				|| self.imbalance().len() == 2)
	}

	/// Accounts and currencies this would use for the first time
	pub fn new_names(&self, history: &History) -> (Vec<String>, Vec<String>) {
		let mut accounts = vec![];
		let mut currencies = vec![];
		for p in &self.postings {
			if !history.is_known_account(&p.account)
				&& !accounts.contains(&p.account)
			{
				accounts.push(p.account.clone());
			}
			if let Some((_, c)) = &p.amount
				&& !history.is_known_currency(c)
				&& !currencies.contains(c)
			{
				currencies.push(c.clone());
			}
		}
		(accounts, currencies)
	}

	/// The entry as ledger text in a file's style, preceded by declarations
	/// for any new names if the ledger declares its names
	pub fn to_text(
		&self,
		style: &FileStyle,
		history: &History,
		precision: &dyn Fn(&str) -> u32,
	) -> String {
		let mut out = String::new();
		if history.uses_declarations() {
			let (accounts, currencies) = self.new_names(history);
			for c in currencies {
				out.push_str(&format!("! {} currency {c}\n", self.date));
			}
			for a in accounts {
				out.push_str(&format!("! {} account {a}\n", self.date));
			}
		}

		out.push_str(&format!("{} {}\n", self.date, self.description));
		let postings: Vec<Posting> = self
			.postings
			.iter()
			.map(|p| posting(p, precision))
			.collect();
		let mut style = style.clone();
		for p in &postings {
			style.fit(p);
		}
		for p in &postings {
			out.push_str(&format!("{}{}\n", style.indent, style.posting(p)));
		}
		out
	}
}

impl Draft {
	/// Checks that text written for this draft reads back as exactly this
	/// entry, so that nothing typed, like a `#`, can quietly change it
	pub fn reads_back(&self, block: &str) -> Result<(), Diagnostic> {
		let mut map = SourceMap::new();
		let file = map.add(crate::syntax::source::SourceFile::new(
			"entry".into(),
			String::new(),
		));
		let mut description = None;
		let mut accounts = vec![];
		for (i, raw) in block.lines().enumerate() {
			let line = lex_line(raw, file, i + 1)?;
			match line.kind {
				LineKind::Header(h) => description = Some(h.description),
				LineKind::Posting(p) => accounts.push(p.account),
				_ => {},
			}
			if line.comment.is_some() {
				return Err(Diagnostic::error(
					"`#` would start a comment, so it can't be part of an entry",
				)
				.help("leave it out, or write something like “No. 1234”"));
			}
		}
		let expected: Vec<&str> =
			self.postings.iter().map(|p| p.account.as_str()).collect();
		if description.as_deref() != Some(self.description.as_str())
			|| accounts != expected
		{
			return Err(Diagnostic::error(
				"The entry wouldn't read back the way it was written",
			)
			.help(
				"check the description and account names for unusual characters",
			));
		}
		Ok(())
	}
}

/// An amount written out at the precision its currency is usually written
/// with, or more if it was typed more precisely
pub fn amount_text(
	value: Quant,
	currency: &str,
	precision: &dyn Fn(&str) -> u32,
) -> String {
	let mut value = value;
	let places = precision(currency).max(value.render_precision());
	value.round(places);
	value.to_string()
}

fn posting(p: &DraftPosting, precision: &dyn Fn(&str) -> u32) -> Posting {
	let zero = Cols { start: 0, end: 0 };
	Posting {
		account: p.account.clone(),
		account_cols: zero,
		amount: p.amount.as_ref().map(|(value, currency)| PostingAmount {
			value: *value,
			value_text: amount_text(*value, currency, precision),
			currency: currency.clone(),
			cols: zero,
			price: None,
			lot: None,
		}),
	}
}

/// The decimal places each currency is written with in a ledger
pub fn precisions(loaded: &Loaded) -> impl Fn(&str) -> u32 + '_ {
	|currency: &str| {
		loaded
			.result
			.max_precision_by_currency
			.get(currency)
			.copied()
			.unwrap_or(DEFAULT_PRECISION)
	}
}

/// The file a new entry goes into, and how that file is laid out
#[derive(Clone, Debug)]
pub struct Target {
	pub path: PathBuf,
	/// Its text when it was read, to detect changes before writing
	pub original: String,
	pub style: FileStyle,
}

impl Target {
	/// The file asked for, or else the one holding the latest entry, so that
	/// new entries land beside recent ones even in a ledger split by year
	pub fn choose(
		explicit: Option<&str>,
		history: &History,
		sources: &SourceMap,
		root: &str,
	) -> Result<Self, Error> {
		let path = match explicit {
			Some(path) => PathBuf::from(path),
			None => match history.latest {
				Some((file, _)) => sources.path(file).to_path_buf(),
				None => PathBuf::from(root),
			},
		};
		if path.as_os_str() == "-" || path.as_os_str() == "<stdin>" {
			bail!(
				"cannot add to a ledger read from stdin; pass a file with -f or --to"
			);
		}
		let original = std::fs::read_to_string(&path)
			.map_err(|e| anyhow!("Could not read `{}`: {e}", path.display()))?;
		let style = detect_style(&original);
		Ok(Self {
			path,
			original,
			style,
		})
	}

	/// Whether the file ends its lines Windows-style
	fn uses_crlf(&self) -> bool {
		self.original.contains("\r\n")
	}

	/// What to append to the file for a block of text: enough newlines that
	/// the block starts after a blank line, with the file's line endings
	pub fn appendix(&self, block: &str) -> String {
		let original = self.original.replace("\r\n", "\n");
		let mut prefix = String::new();
		if !original.trim().is_empty() {
			if !original.ends_with('\n') {
				prefix.push('\n');
			}
			if !original.ends_with("\n\n") {
				prefix.push('\n');
			}
		}
		let text = format!("{prefix}{block}");
		if self.uses_crlf() {
			text.replace('\n', "\r\n")
		} else {
			text
		}
	}

	/// The line number the block's first line will have once appended
	pub fn first_new_line(&self, appendix: &str) -> usize {
		let original = self.original.replace("\r\n", "\n");
		let appendix = appendix.replace("\r\n", "\n");
		let existing = original.lines().count();
		let leading = appendix.len() - appendix.trim_start_matches('\n').len();
		let gap = if original.ends_with('\n') || original.is_empty() {
			leading
		} else {
			leading.saturating_sub(1)
		};
		existing + gap + 1
	}

	/// Checks the ledger with the text appended, in memory. Returns the
	/// loaded ledger, or the error, along with the sources to show it with.
	pub fn validate(
		&self,
		options: &LoadOptions,
		appendix: &str,
	) -> (Result<Loaded, Error>, SourceMap) {
		let mut options = options.clone();
		options.overlay = Some(Overlay {
			path: self.path.clone(),
			appended: appendix.to_string(),
		});
		let mut sources = SourceMap::new();
		let result = load_ledger(&options, &mut sources);
		(result, sources)
	}

	/// Warnings that are about the appended text
	pub fn new_warnings<'a>(
		&self,
		loaded: &'a Loaded,
		sources: &SourceMap,
		appendix: &str,
	) -> Vec<&'a Diagnostic> {
		let first = self.first_new_line(appendix);
		let file: Option<FileId> = sources
			.files()
			.find(|(_, f)| same_file(&f.path, &self.path))
			.map(|(id, _)| id);
		loaded
			.ledger
			.warnings
			.iter()
			.filter(|w| {
				w.span.is_some_and(|s| {
					Some(s.file) == file && s.last_line >= first
				})
			})
			.collect()
	}

	/// Appends the text, as long as the file has not changed since it was
	/// read. Returns the line the block starts on.
	pub fn append(&self, appendix: &str) -> Result<usize, Error> {
		let current = std::fs::read_to_string(&self.path)?;
		if current != self.original {
			bail!(
				"`{}` changed while the entry was being written; nothing was saved",
				self.path.display()
			);
		}
		let mut file =
			std::fs::OpenOptions::new().append(true).open(&self.path)?;
		file.write_all(appendix.as_bytes())?;
		file.flush()?;
		Ok(self.first_new_line(appendix))
	}
}

fn same_file(a: &Path, b: &Path) -> bool {
	match (std::fs::canonicalize(a), std::fs::canonicalize(b)) {
		(Ok(a), Ok(b)) => a == b,
		_ => a == b,
	}
}

/// A file's layout, learned from whichever of its lines make sense
pub fn detect_style(text: &str) -> FileStyle {
	let mut map = SourceMap::new();
	let file = map.add(crate::syntax::source::SourceFile::new(
		"x".into(),
		String::new(),
	));
	let lines: Vec<(&str, Line)> = text
		.lines()
		.enumerate()
		.filter_map(|(i, raw)| {
			lex_line(raw, file, i + 1).ok().map(|l| (raw, l))
		})
		.collect();
	FileStyle::detect(lines.iter().map(|(raw, l)| (*raw, l)))
}

#[cfg(test)]
mod tests {
	use super::*;

	fn draft() -> Draft {
		Draft {
			date: Date::from_str("2024-03-01").unwrap(),
			description: "Tim Hortons".into(),
			postings: vec![
				DraftPosting::new(
					"Expenses:Food:Coffee",
					Some((Quant::from_str("4.5").unwrap(), "CAD")),
				),
				DraftPosting::new("Liabilities:Visa", None),
			],
			notes: vec![],
		}
	}

	#[test]
	fn test_text_matches_the_file() {
		let style = detect_style(
			"2024-01-01 X\n\tAssets:A    1,000.00 CAD\n\tAssets:B\n",
		);
		let text = draft().to_text(&style, &History::default(), &|_| 2);
		assert_eq!(
			text,
			"2024-03-01 Tim Hortons\n\tExpenses:Food:Coffee      4.50 CAD\n\tLiabilities:Visa\n"
		);
	}

	#[test]
	fn test_balance() {
		let mut d = draft();
		assert!(d.is_balanced());
		d.postings[1].amount =
			Some((Quant::from_str("-4.5").unwrap(), "CAD".into()));
		assert!(d.is_balanced());
		assert!(d.imbalance().is_empty());
		d.postings[1].amount =
			Some((Quant::from_str("-4").unwrap(), "CAD".into()));
		assert!(!d.is_balanced());
		d.postings[1].amount =
			Some((Quant::from_str("-3").unwrap(), "USD".into()));
		assert!(d.is_balanced(), "two currencies convert implicitly");
	}

	#[test]
	fn test_reads_back() {
		let d = draft();
		let text =
			d.to_text(&FileStyle::default(), &History::default(), &|_| 2);
		assert!(d.reads_back(&text).is_ok());

		let mut hashed = draft();
		hashed.description = "Order #1234".into();
		let text =
			hashed.to_text(&FileStyle::default(), &History::default(), &|_| 2);
		assert!(hashed.reads_back(&text).unwrap_err().message.contains('#'));
	}

	#[test]
	fn test_typed_precision_is_kept() {
		let value = Quant::from_str("0.123456789012345678").unwrap();
		assert_eq!(amount_text(value, "ETH", &|_| 2), "0.123456789012345678");
		assert_eq!(
			amount_text(Quant::from_frac(100, 3), "USD", &|_| 2),
			"33.33"
		);
	}

	#[test]
	fn test_windows_line_endings_are_kept() {
		let target = Target {
			path: "x".into(),
			original: "a\r\n".into(),
			style: FileStyle::default(),
		};
		let appendix = target.appendix("B\nC\n");
		assert_eq!(appendix, "\r\nB\r\nC\r\n");
		assert_eq!(target.first_new_line(&appendix), 3);
		let blank = Target {
			original: "a\r\n\r\n".into(),
			..target
		};
		assert_eq!(blank.appendix("B\n"), "B\r\n");
	}

	#[test]
	fn test_appendix_separates_with_a_blank_line() {
		let target = |original: &str| Target {
			path: "x".into(),
			original: original.into(),
			style: FileStyle::default(),
		};
		assert_eq!(target("a\n").appendix("B\n"), "\nB\n");
		assert_eq!(target("a").appendix("B\n"), "\n\nB\n");
		assert_eq!(target("a\n\n").appendix("B\n"), "B\n");
		assert_eq!(target("").appendix("B\n"), "B\n");
		assert_eq!(target("a\n").first_new_line("\nB\n"), 3);
		assert_eq!(target("a").first_new_line("\n\nB\n"), 3);
		assert_eq!(target("").first_new_line("B\n"), 1);
	}
}
