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

//! Classifies single lines of ledger text and breaks them into their parts.
//!
//! This is the one place that knows the ledger syntax. The ledger builder,
//! the formatter and the entry composer all work from what it produces, so
//! they can never disagree about what a line means.

use crate::diagnostics::{Diagnostic, closest};
use crate::syntax::source::{FileId, Span};
use crate::util::date::Date;
use crate::util::quant::Quant;

/// One line of a ledger file, classified and broken into its parts.
#[derive(Clone, Debug, PartialEq)]
pub struct Line {
	pub kind: LineKind,
	/// Trailing comment, from its `#` to the end of the line
	pub comment: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum LineKind {
	/// Nothing but whitespace. Ends the current entry.
	Blank,
	/// Nothing but a comment. Does not end the current entry.
	CommentOnly,
	Include(Include),
	Directive(DirectiveLine),
	Header(Header),
	/// A `//` line attached to the surrounding entry
	Reference(String),
	Posting(Box<Posting>),
}

/// Byte columns within a line, for pointing at exactly what went wrong
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Cols {
	pub start: usize,
	pub end: usize,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Include {
	pub path: String,
	pub cols: Cols,
}

#[derive(Clone, Debug, PartialEq)]
pub struct DirectiveLine {
	pub date: Date,
	pub directive: Directive,
	/// Columns of the directive's arguments, after its keyword
	pub args: Cols,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Directive {
	Account(String),
	Open(String),
	Close(String),
	Currency(String),
	Clear(String),
	Worthless(String),
	Rate {
		base: String,
		quote: String,
		rate: Quant,
		rate_text: String,
	},
}

impl Directive {
	pub const KEYWORDS: [&str; 7] = [
		"account",
		"open",
		"close",
		"currency",
		"clear",
		"worthless",
		"rate",
	];

	pub fn keyword(&self) -> &'static str {
		match self {
			Directive::Account(_) => "account",
			Directive::Open(_) => "open",
			Directive::Close(_) => "close",
			Directive::Currency(_) => "currency",
			Directive::Clear(_) => "clear",
			Directive::Worthless(_) => "worthless",
			Directive::Rate { .. } => "rate",
		}
	}

	/// The directive's arguments, as they should be written
	pub fn args(&self) -> String {
		match self {
			Directive::Account(a)
			| Directive::Open(a)
			| Directive::Close(a)
			| Directive::Currency(a)
			| Directive::Clear(a)
			| Directive::Worthless(a) => a.clone(),
			Directive::Rate {
				base,
				quote,
				rate_text,
				..
			} => format!("{base} {quote} {rate_text}"),
		}
	}
}

#[derive(Clone, Debug, PartialEq)]
pub struct Header {
	pub date: Date,
	pub description: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Posting {
	pub account: String,
	pub account_cols: Cols,
	/// None for the one line of an entry that balances all the others
	pub amount: Option<PostingAmount>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct PostingAmount {
	pub value: Quant,
	/// The amount exactly as written, e.g. `1,000.00`
	pub value_text: String,
	pub currency: String,
	/// Columns spanning the amount and currency
	pub cols: Cols,
	pub price: Option<Price>,
	pub lot: Option<LotSpec>,
}

/// An inline conversion, `@ 1.3 CAD` (per unit) or `@@ 130 CAD` (total)
#[derive(Clone, Debug, PartialEq)]
pub struct Price {
	pub is_total: bool,
	pub value: Quant,
	pub value_text: String,
	pub currency: String,
	pub cols: Cols,
}

/// A lot's cost basis and optional name, `{ 234.56 USD, "lot A" }`
#[derive(Clone, Debug, PartialEq)]
pub struct LotSpec {
	pub cost: Quant,
	pub cost_text: String,
	pub currency: String,
	pub name: Option<String>,
	pub cols: Cols,
}

/// Lexes one line of ledger text. `number` counts from 1.
pub fn lex_line(
	raw: &str,
	file: FileId,
	number: usize,
) -> Result<Line, Diagnostic> {
	LineLexer { raw, file, number }.lex()
}

/// True for text shaped like a date (digits-digits-digits), valid or not.
/// Such a word at the start of a line always begins an entry.
pub fn is_date_like(word: &str) -> bool {
	let parts: Vec<&str> = word.split('-').collect();
	parts.len() == 3
		&& parts
			.iter()
			.all(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit()))
}

/// True for text that reads as an amount: digits with an optional sign,
/// thousands separators and decimal point.
pub fn is_number(word: &str) -> bool {
	let digits = word.strip_prefix(['-', '+']).unwrap_or(word);
	digits.bytes().any(|b| b.is_ascii_digit())
		&& digits
			.bytes()
			.all(|b| b.is_ascii_digit() || b == b',' || b == b'.')
		&& digits.bytes().filter(|&b| b == b'.').count() <= 1
}

/// Parses an amount as written in a ledger, allowing thousands separators.
pub fn parse_number(word: &str) -> Result<Quant, anyhow::Error> {
	Quant::from_str(&word.replace(',', ""))
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Tok<'a> {
	Word(&'a str),
	Quoted(&'a str),
	Open,
	Close,
}

struct LineLexer<'a> {
	raw: &'a str,
	file: FileId,
	number: usize,
}

impl<'a> LineLexer<'a> {
	fn span(&self, start: usize, end: usize) -> Span {
		Span::new(self.file, self.number, start, end)
	}

	fn error(
		&self,
		message: impl Into<String>,
		start: usize,
		end: usize,
	) -> Diagnostic {
		Diagnostic::error(message).at(self.span(start, end))
	}

	fn lex(&self) -> Result<Line, Diagnostic> {
		let (content_end, comment) = match self.raw.find('#') {
			Some(i) => (i, Some(self.raw[i..].trim_end().to_string())),
			None => (self.raw.len(), None),
		};
		let before_comment = &self.raw[..content_end];
		let offset = before_comment.len() - before_comment.trim_start().len();
		let content = before_comment.trim();

		let kind = if content.is_empty() {
			if comment.is_some() {
				LineKind::CommentOnly
			} else {
				LineKind::Blank
			}
		} else if let Some(rest) = strip_keyword(content, "include") {
			self.include(content, rest, offset)?
		} else if let Some(rest) = content.strip_prefix('!') {
			self.directive(rest, offset + 1)?
		} else if let Some(rest) = content.strip_prefix("//") {
			LineKind::Reference(rest.trim().to_string())
		} else if content.split_whitespace().next().is_some_and(is_date_like) {
			self.header(content, offset)?
		} else {
			LineKind::Posting(Box::new(self.posting(content, offset)?))
		};

		Ok(Line { kind, comment })
	}

	fn include(
		&self,
		content: &str,
		rest: &str,
		offset: usize,
	) -> Result<LineKind, Diagnostic> {
		let path = rest.trim();
		let start = offset + (content.len() - rest.trim_start().len());
		let path = path
			.strip_prefix('"')
			.and_then(|p| p.strip_suffix('"'))
			.unwrap_or(path);
		if path.is_empty() {
			return Err(self
				.error("Include is missing a file path", offset, offset + 7)
				.help(
					"write the path after the keyword, like `include 2024.ledr`",
				));
		}
		Ok(LineKind::Include(Include {
			path: path.to_string(),
			cols: Cols {
				start,
				end: offset + content.len(),
			},
		}))
	}

	fn date(&self, text: &str, start: usize) -> Result<Date, Diagnostic> {
		Date::from_str(text).map_err(|_| {
			self.error(
				format!("`{text}` is not a valid date"),
				start,
				start + text.len(),
			)
			.help("dates are written YYYY-MM-DD, like 2024-11-24")
		})
	}

	fn header(
		&self,
		content: &str,
		offset: usize,
	) -> Result<LineKind, Diagnostic> {
		let date_text = content.split_whitespace().next().unwrap_or_default();
		let date = self.date(date_text, offset)?;
		let description = content[date_text.len()..].trim();
		if description.is_empty() {
			return Err(self
				.error(
					format!("Entry dated {date} has no description"),
					offset,
					offset + content.len(),
				)
				.help(
					"write a description after the date, like `2024-11-24 Paycheck`",
				));
		}
		Ok(LineKind::Header(Header {
			date,
			description: description.to_string(),
		}))
	}

	fn directive(
		&self,
		content: &str,
		offset: usize,
	) -> Result<LineKind, Diagnostic> {
		let words = words(content, offset);
		let whole = |message: String| {
			self.error(message, offset - 1, offset + content.len())
		};

		let Some(&(date_text, date_start, _)) = words.first() else {
			return Err(whole("Directive is empty".into()).help(
				"directives look like `! 2024-11-24 account Assets:Cash`",
			));
		};
		let date = self.date(date_text, date_start)?;

		let Some(&(keyword, kw_start, kw_end)) = words.get(1) else {
			return Err(whole(format!("Directive on {date} has no keyword"))
				.help(format!(
					"expected one of: {}",
					Directive::KEYWORDS.join(", ")
				)));
		};

		let args = &words[2..];
		let arg_cols = Cols {
			start: args.first().map_or(kw_end, |a| a.1),
			end: args.last().map_or(kw_end, |a| a.2),
		};
		let usage = |example: &str| {
			whole(format!("Invalid `{keyword}` directive"))
				.help(format!("write it like `! {date} {keyword} {example}`"))
		};

		let single = |example: &str| match args {
			[(arg, _, _)] => Ok(arg.to_string()),
			_ => Err(usage(example)),
		};
		let account = |example: &str| {
			let account = single(example)?;
			if !account.contains(':') {
				return Err(self
					.error(
						"Top level accounts cannot be used on their own",
						arg_cols.start,
						arg_cols.end,
					)
					.help(format!(
						"use a sub-account, like `{account}:Cash`"
					)));
			}
			Ok(account)
		};

		let directive = match keyword {
			"account" => Directive::Account(account("Assets:Cash")?),
			"open" => Directive::Open(account("Assets:Cash")?),
			"close" => Directive::Close(account("Assets:Cash")?),
			"currency" => Directive::Currency(single("USD")?),
			"clear" => Directive::Clear(single("USD")?),
			"worthless" => Directive::Worthless(single("USD")?),
			"rate" => match args {
				[(base, _, _), (quote, _, _), (rate, rate_start, rate_end)] => {
					let value = parse_number(rate).map_err(|e| {
						self.error(e.to_string(), *rate_start, *rate_end)
					})?;
					Directive::Rate {
						base: base.to_string(),
						quote: quote.to_string(),
						rate: value,
						rate_text: rate.to_string(),
					}
				},
				_ => return Err(usage("USD CAD 1.3")),
			},
			other => {
				let mut error = self.error(
					format!("Unknown directive `{other}`"),
					kw_start,
					kw_end,
				);
				error = match closest(other, Directive::KEYWORDS) {
					Some(suggestion) => {
						error.help(format!("did you mean `{suggestion}`?"))
					},
					None => error.help(format!(
						"expected one of: {}",
						Directive::KEYWORDS.join(", ")
					)),
				};
				return Err(error);
			},
		};

		Ok(LineKind::Directive(DirectiveLine {
			date,
			directive,
			args: arg_cols,
		}))
	}

	fn tokenize(
		&self,
		s: &'a str,
		offset: usize,
	) -> Result<Vec<(Tok<'a>, usize, usize)>, Diagnostic> {
		let mut tokens = vec![];
		let mut i = 0;
		while let Some(c) = s[i..].chars().next() {
			let at = offset + i;
			match c {
				c if c.is_whitespace() => {
					i += c.len_utf8();
					continue;
				},
				'{' => tokens.push((Tok::Open, at, at + 1)),
				'}' => tokens.push((Tok::Close, at, at + 1)),
				'"' => {
					let Some(len) = s[i + 1..].find('"') else {
						return Err(self
							.error("Unclosed quote", at, offset + s.len())
							.help("lot names are quoted, like \"lot A\""));
					};
					tokens.push((
						Tok::Quoted(&s[i + 1..i + 1 + len]),
						at,
						at + len + 2,
					));
					i += len + 2;
					continue;
				},
				_ => {
					let len = s[i..]
						.find(|c: char| {
							c.is_whitespace() || matches!(c, '{' | '}' | '"')
						})
						.unwrap_or(s.len() - i);
					tokens.push((Tok::Word(&s[i..i + len]), at, at + len));
					i += len;
					continue;
				},
			}
			i += 1;
		}
		Ok(tokens)
	}

	fn number(
		&self,
		token: Option<&(Tok<'a>, usize, usize)>,
		what: &str,
		after: usize,
	) -> Result<(Quant, String, usize, usize), Diagnostic> {
		match token {
			Some(&(Tok::Word(w), start, end)) if is_number(w) => {
				let value = parse_number(w)
					.map_err(|e| self.error(e.to_string(), start, end))?;
				Ok((value, w.to_string(), start, end))
			},
			Some(&(_, start, end)) => {
				Err(self.error(format!("Expected {what} here"), start, end))
			},
			None => {
				Err(self.error(format!("Missing {what}"), after, after + 1))
			},
		}
	}

	fn currency(
		&self,
		token: Option<&(Tok<'a>, usize, usize)>,
		after_text: &str,
		after: usize,
	) -> Result<(String, usize), Diagnostic> {
		match token {
			Some(&(Tok::Word(w), _, end))
				if !is_number(w) && !w.starts_with('@') =>
			{
				// A stray trailing comma was always ignored, so keep ignoring it
				let currency = w.trim_end_matches(',');
				Ok((currency.to_string(), end - (w.len() - currency.len())))
			},
			Some(&(_, start, end)) => Err(self
				.error("Expected a currency here".to_string(), start, end)
				.help(format!(
					"write the currency after the amount, like `{after_text} USD`"
				))),
			None => Err(self
				.error(
					format!("Missing a currency after `{after_text}`"),
					after,
					after + 1,
				)
				.help(format!(
					"write the currency after the amount, like `{after_text} USD`"
				))),
		}
	}

	fn posting(
		&self,
		content: &'a str,
		offset: usize,
	) -> Result<Posting, Diagnostic> {
		let tokens = self.tokenize(content, offset)?;

		let (account, a_start, a_end) = match tokens.first() {
			Some(&(Tok::Word(w), start, end)) => (w.to_string(), start, end),
			Some(&(_, start, end)) => {
				return Err(self.error("Expected an account name", start, end));
			},
			None => unreachable!("posting lines are never empty"),
		};
		let account_cols = Cols {
			start: a_start,
			end: a_end,
		};

		if tokens.len() == 1 {
			return Ok(Posting {
				account,
				account_cols,
				amount: None,
			});
		}

		// A common slip: a space inside an account name
		if let Some(&(Tok::Word(w), start, end)) = tokens.get(1)
			&& !is_number(w)
		{
			let mut error = self.error(
				format!("Expected an amount after `{account}`, found `{w}`"),
				start,
				end,
			);
			error = if tokens.get(2).is_some_and(
				|(t, _, _)| matches!(t, Tok::Word(n) if is_number(n)),
			) {
				if w.chars().all(|c| c.is_ascii_uppercase()) {
					error.help(
						"amounts come before their currency, like `100 USD`",
					)
				} else {
					error.help("account names cannot contain spaces")
				}
			} else {
				error.help("postings look like `Assets:Cash  100 USD`")
			};
			return Err(error);
		}

		let (value, value_text, v_start, v_end) =
			self.number(tokens.get(1), "an amount", a_end)?;
		let (currency, c_end) =
			self.currency(tokens.get(2), &value_text, v_end)?;

		let mut amount = PostingAmount {
			value,
			value_text,
			currency,
			cols: Cols {
				start: v_start,
				end: c_end,
			},
			price: None,
			lot: None,
		};

		let rest = &tokens[3..];
		match rest.first() {
			None => {},
			Some(&(Tok::Word(op @ ("@" | "@@")), op_start, op_end)) => {
				let (value, value_text, _, p_end) =
					self.number(rest.get(1), "a price", op_end)?;
				let (currency, c_end) =
					self.currency(rest.get(2), &value_text, p_end)?;
				if let Some(&(_, start, _)) = rest.get(3) {
					return Err(self.error(
						"Unexpected text after the price",
						start,
						offset + content.len(),
					));
				}
				if op == "@@" && amount.value.is_zero() {
					return Err(self.error(
						"A total price (`@@`) needs a nonzero amount",
						v_start,
						c_end,
					));
				}
				amount.price = Some(Price {
					is_total: op == "@@",
					value,
					value_text,
					currency,
					cols: Cols {
						start: op_start,
						end: c_end,
					},
				});
			},
			Some(&(Tok::Open, open_start, open_end)) => {
				amount.lot = Some(self.lot(
					rest,
					open_start,
					open_end,
					offset + content.len(),
				)?);
			},
			Some(&(_, start, end)) => {
				return Err(self
					.error("Unexpected text after the amount", start, end)
					.help("an amount may be followed by a price like `@ 1.3 CAD` or a lot like `{ 234.56 USD }`"));
			},
		}

		Ok(Posting {
			account,
			account_cols,
			amount: Some(amount),
		})
	}

	fn lot(
		&self,
		rest: &[(Tok<'a>, usize, usize)],
		open_start: usize,
		open_end: usize,
		line_end: usize,
	) -> Result<LotSpec, Diagnostic> {
		let Some(close) = rest.iter().position(|(t, _, _)| *t == Tok::Close)
		else {
			return Err(self.error("Unclosed `{`", open_start, open_end).help(
				"lots look like `{ 234.56 USD }` or `{ 234.56 USD, \"lot A\" }`",
			));
		};
		if let Some(&(_, start, _)) = rest.get(close + 1) {
			return Err(self.error(
				"Unexpected text after the lot",
				start,
				line_end,
			));
		}

		// Commas between the parts are optional decoration
		let inner: Vec<(Tok<'a>, usize, usize)> = rest[1..close]
			.iter()
			.filter_map(|&(t, s, e)| match t {
				Tok::Word(",") => None,
				Tok::Word(w) if w.ends_with(',') && w.len() > 1 => {
					Some((Tok::Word(&w[..w.len() - 1]), s, e - 1))
				},
				_ => Some((t, s, e)),
			})
			.collect();

		let (cost, cost_text, _, cost_end) =
			self.number(inner.first(), "a cost basis", open_end)?;
		let (currency, c_end) =
			self.currency(inner.get(1), &cost_text, cost_end)?;
		let name = match inner.get(2) {
			None => None,
			Some(&(Tok::Quoted(n) | Tok::Word(n), _, _)) => Some(n.to_string()),
			Some(&(_, start, end)) => {
				return Err(self.error("Expected a lot name", start, end));
			},
		};
		if let Some(&(_, start, end)) = inner.get(3) {
			return Err(self
				.error("Unexpected text in the lot", start, end)
				.help("lot names with spaces need quotes, like \"lot A\""));
		}
		let _ = c_end;

		Ok(LotSpec {
			cost,
			cost_text,
			currency,
			name,
			cols: Cols {
				start: open_start,
				end: rest[close].2,
			},
		})
	}
}

/// Strips `keyword` from the start of `s` if it is followed by whitespace or
/// nothing at all.
fn strip_keyword<'s>(s: &'s str, keyword: &str) -> Option<&'s str> {
	let rest = s.strip_prefix(keyword)?;
	(rest.is_empty() || rest.starts_with(char::is_whitespace)).then_some(rest)
}

/// Whitespace-separated words with their byte columns
fn words(s: &str, offset: usize) -> Vec<(&str, usize, usize)> {
	s.split_whitespace()
		.map(|w| {
			let start = offset + (w.as_ptr() as usize - s.as_ptr() as usize);
			(w, start, start + w.len())
		})
		.collect()
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::syntax::source::SourceMap;

	fn file() -> FileId {
		SourceMap::new().add(crate::syntax::source::SourceFile::new(
			"test".into(),
			String::new(),
		))
	}

	fn lex(s: &str) -> LineKind {
		lex_line(s, file(), 1).unwrap().kind
	}

	fn lex_err(s: &str) -> Diagnostic {
		lex_line(s, file(), 1).unwrap_err()
	}

	fn posting(s: &str) -> Posting {
		match lex(s) {
			LineKind::Posting(p) => *p,
			other => panic!("expected a posting, got {other:?}"),
		}
	}

	#[test]
	fn test_blank_and_comments() {
		assert_eq!(lex(""), LineKind::Blank);
		assert_eq!(lex("   \t "), LineKind::Blank);
		assert_eq!(lex("# hello"), LineKind::CommentOnly);
		assert_eq!(lex("    # hello"), LineKind::CommentOnly);
		let line = lex_line("  Assets:Cash  # note", file(), 1).unwrap();
		assert_eq!(line.comment.as_deref(), Some("# note"));
	}

	#[test]
	fn test_header_keeps_punctuation() {
		match lex("2024-11-24 Paycheck, Google, Inc.") {
			LineKind::Header(h) => {
				assert_eq!(h.description, "Paycheck, Google, Inc.");
			},
			other => panic!("{other:?}"),
		}
		match lex("2024-11-24\tTabbed") {
			LineKind::Header(h) => assert_eq!(h.description, "Tabbed"),
			other => panic!("{other:?}"),
		}
	}

	#[test]
	fn test_header_errors() {
		let e = lex_err("2024-13-01 Nope");
		assert!(e.message.contains("not a valid date"));
		assert_eq!(e.span.unwrap().start, 0);
		assert_eq!(e.span.unwrap().end, 10);
		assert!(lex_err("2024-01-01").message.contains("no description"));
	}

	#[test]
	fn test_postings() {
		let p = posting("    Assets:Cash   1,000.00 USD");
		assert_eq!(p.account, "Assets:Cash");
		assert_eq!(p.account_cols, Cols { start: 4, end: 15 });
		let amount = p.amount.unwrap();
		assert_eq!(amount.value, Quant::from_i128(1000));
		assert_eq!(amount.value_text, "1,000.00");
		assert_eq!(amount.currency, "USD");

		assert!(posting("  Income:Salary").amount.is_none());

		let price = posting("Expenses:Hw 12.5 USD @ 1.3 CAD")
			.amount
			.unwrap()
			.price
			.unwrap();
		assert!(!price.is_total);
		assert_eq!(price.currency, "CAD");
		assert!(
			posting("Expenses:Hw 12.5 USD @@ 16.25 CAD")
				.amount
				.unwrap()
				.price
				.unwrap()
				.is_total
		);
	}

	#[test]
	fn test_trailing_comma_after_currency_is_ignored() {
		let amount = posting("Assets:A 10 USD,").amount.unwrap();
		assert_eq!(amount.currency, "USD");
		assert_eq!(amount.cols, Cols { start: 9, end: 15 });
	}

	#[test]
	fn test_lots() {
		for text in [
			r#"Assets:A 30 AAPL { 234.56 USD, "lot A" }"#,
			r#"Assets:A 30 AAPL { 234.56 USD "lot A" }"#,
			r#"Assets:A 30 AAPL {234.56 USD,"lot A"}"#,
		] {
			let lot = posting(text).amount.unwrap().lot.unwrap();
			assert_eq!(lot.cost, Quant::from_frac(23456, 100), "{text}");
			assert_eq!(lot.currency, "USD", "{text}");
			assert_eq!(lot.name.as_deref(), Some("lot A"), "{text}");
		}
		let lot = posting("Assets:A -3 AAPL { 1,234.56 USD }")
			.amount
			.unwrap()
			.lot
			.unwrap();
		assert_eq!(lot.cost, Quant::from_frac(123456, 100));
		assert_eq!(lot.name, None);
		let named = posting("Assets:A 3 AAPL { 1 USD lotA }");
		assert_eq!(named.amount.unwrap().lot.unwrap().name.unwrap(), "lotA");
	}

	#[test]
	fn test_posting_errors() {
		let e = lex_err("Assets:Cash 100");
		assert!(e.message.contains("Missing a currency"), "{}", e.message);
		let e = lex_err("Assets:Cash USD 100");
		assert!(e.help.unwrap().contains("before their currency"));
		let e = lex_err("Expenses:Eating Out 10 USD");
		assert!(e.help.unwrap().contains("cannot contain spaces"));
		assert!(lex_err("Assets:A 1 X { 2 USD").message.contains("Unclosed"));
		assert!(lex_err("Assets:A 1 X ~").message.contains("Unexpected"));
		assert!(lex_err("Assets:A 0 X @@ 5 USD").message.contains("nonzero"));
	}

	#[test]
	fn test_directives() {
		match lex("! 2024-11-24 rate USD CAD 1,300.5") {
			LineKind::Directive(d) => assert_eq!(
				d.directive,
				Directive::Rate {
					base: "USD".into(),
					quote: "CAD".into(),
					rate: Quant::from_frac(26010, 20),
					rate_text: "1,300.5".into(),
				}
			),
			other => panic!("{other:?}"),
		}
		match lex("!2024-11-24 account Assets:Cash") {
			LineKind::Directive(d) => {
				assert_eq!(
					d.directive,
					Directive::Account("Assets:Cash".into())
				);
			},
			other => panic!("{other:?}"),
		}
		assert!(
			lex_err("! 2024-11-24 account Assets")
				.message
				.contains("Top level")
		);
		let e = lex_err("! 2024-11-24 acount Assets:Cash");
		assert_eq!(e.help.unwrap(), "did you mean `account`?");
		assert!(lex_err("! 2024-11-24 rate USD").message.contains("Invalid"));
	}

	#[test]
	fn test_includes_and_references() {
		match lex(r#"include "my file.ledr""#) {
			LineKind::Include(i) => assert_eq!(i.path, "my file.ledr"),
			other => panic!("{other:?}"),
		}
		match lex("include 2024.ledr  # comment") {
			LineKind::Include(i) => assert_eq!(i.path, "2024.ledr"),
			other => panic!("{other:?}"),
		}
		assert!(lex_err("include").message.contains("missing"));
		assert_eq!(
			lex("// INV-1, paid"),
			LineKind::Reference("INV-1, paid".into())
		);
		assert_eq!(lex("//"), LineKind::Reference(String::new()));
	}
}
