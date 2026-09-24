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

//! Amounts as people type them: with arithmetic and an optional currency.

use crate::syntax::lexer::parse_number;
use crate::util::quant::Quant;
use anyhow::{Error, anyhow, bail};

/// An amount typed at a prompt, like `12.50`, `4.50 CAD`, `CAD 4.50`,
/// `12.50 + 3.20 * 2` or `(90 / 3) CAD`.
#[derive(Clone, Debug, PartialEq)]
pub struct Typed {
	pub value: Quant,
	pub currency: Option<String>,
	/// Whether it was a calculation, rather than a plain number
	pub computed: bool,
}

/// Currency symbols that stand for a code, when typed before a number
const SYMBOLS: [(char, &str); 4] =
	[('$', "USD"), ('€', "EUR"), ('£', "GBP"), ('¥', "JPY")];

/// Parses a typed amount. Anything alphabetic before or after the
/// arithmetic is taken as the currency.
pub fn parse(input: &str) -> Result<Typed, Error> {
	let input = input.trim();
	if input.is_empty() {
		bail!("no amount given");
	}

	let mut currency = None;
	let mut body = input.to_string();

	// A trailing or leading currency code
	let is_code = |w: &str| {
		!w.is_empty()
			&& w.chars().all(|c| c.is_alphabetic() || c == '_' || c == '.')
			&& w.chars().any(char::is_alphabetic)
	};
	if let Some((rest, last)) = body.rsplit_once(char::is_whitespace)
		&& is_code(last)
	{
		currency = Some(last.to_string());
		body = rest.trim().to_string();
	}
	if currency.is_none()
		&& let Some((first, rest)) = body.split_once(char::is_whitespace)
		&& is_code(first)
	{
		currency = Some(first.to_string());
		body = rest.trim().to_string();
	}
	if currency.is_none() {
		// Glued on, like 4.50CAD
		let split = body.find(|c: char| c.is_alphabetic());
		if let Some(i) = split.filter(|&i| i > 0 && is_code(&body[i..])) {
			currency = Some(body[i..].to_string());
			body = body[..i].trim().to_string();
		}
	}
	for (symbol, code) in SYMBOLS {
		if let Some(rest) = body.strip_prefix(symbol) {
			currency.get_or_insert_with(|| code.to_string());
			body = rest.trim().to_string();
		} else if let Some(rest) =
			body.strip_prefix('-').and_then(|b| b.strip_prefix(symbol))
		{
			currency.get_or_insert_with(|| code.to_string());
			body = format!("-{}", rest.trim());
		}
	}

	let mut parser = Parser {
		chars: body.chars().filter(|c| !c.is_whitespace()).collect(),
		i: 0,
		operations: 0,
	};
	let value = parser.expression()?;
	if parser.i < parser.chars.len() {
		bail!("`{input}` is not an amount I understand");
	}
	Ok(Typed {
		value,
		currency,
		computed: parser.operations > 0,
	})
}

struct Parser {
	chars: Vec<char>,
	i: usize,
	operations: usize,
}

impl Parser {
	fn peek(&self) -> Option<char> {
		self.chars.get(self.i).copied()
	}

	fn expression(&mut self) -> Result<Quant, Error> {
		let mut value = self.term()?;
		while let Some(op @ ('+' | '-')) = self.peek() {
			self.i += 1;
			self.operations += 1;
			let rhs = self.term()?;
			value = if op == '+' { value + rhs } else { value - rhs };
		}
		Ok(value)
	}

	fn term(&mut self) -> Result<Quant, Error> {
		let mut value = self.factor()?;
		while let Some(op @ ('*' | 'x' | '×' | '/' | '÷')) = self.peek() {
			self.i += 1;
			self.operations += 1;
			let rhs = self.factor()?;
			value = if matches!(op, '/' | '÷') {
				if rhs.is_zero() {
					bail!("cannot divide by zero");
				}
				value / rhs
			} else {
				value * rhs
			};
		}
		Ok(value)
	}

	fn factor(&mut self) -> Result<Quant, Error> {
		match self.peek() {
			Some('-') => {
				self.i += 1;
				Ok(-self.factor()?)
			},
			Some('+') => {
				self.i += 1;
				self.factor()
			},
			Some('(') => {
				self.i += 1;
				let value = self.expression()?;
				if self.peek() != Some(')') {
					bail!("missing a closing parenthesis");
				}
				self.i += 1;
				Ok(value)
			},
			Some(c) if c.is_ascii_digit() || c == '.' => {
				let start = self.i;
				while self
					.peek()
					.is_some_and(|c| c.is_ascii_digit() || c == '.' || c == ',')
				{
					self.i += 1;
				}
				let text: String = self.chars[start..self.i].iter().collect();
				parse_number(&text)
					.map_err(|_| anyhow!("`{text}` is not a number"))
			},
			Some(c) => bail!("unexpected `{c}`"),
			None => bail!("the amount is incomplete"),
		}
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	fn value(s: &str) -> String {
		let mut q = parse(s).unwrap().value;
		q.set_render_precision(2, true);
		q.to_string()
	}

	#[test]
	fn test_plain_amounts() {
		assert_eq!(value("12.5"), "12.50");
		assert_eq!(value("1,234.56"), "1,234.56");
		assert_eq!(value("-4"), "-4.00");
		assert!(!parse("12.5").unwrap().computed);
	}

	#[test]
	fn test_arithmetic() {
		assert_eq!(value("12.50 + 3.20 * 2"), "18.90");
		assert_eq!(value("(10 + 2) / 3"), "4.00");
		assert_eq!(value("100/3"), "33.33");
		assert_eq!(value("3x4"), "12.00");
		assert_eq!(value("-(2+3)"), "-5.00");
		assert!(parse("1+2").unwrap().computed);
		assert!(parse("1/0").is_err());
		assert!(parse("(1+2").is_err());
		assert!(parse("12..5").is_err());
		assert!(parse("abc").is_err());
	}

	#[test]
	fn test_currencies() {
		assert_eq!(parse("4.50 CAD").unwrap().currency.as_deref(), Some("CAD"));
		assert_eq!(parse("CAD 4.50").unwrap().currency.as_deref(), Some("CAD"));
		assert_eq!(parse("4.50CAD").unwrap().currency.as_deref(), Some("CAD"));
		assert_eq!(parse("$4.50").unwrap().currency.as_deref(), Some("USD"));
		assert_eq!(parse("€ 3").unwrap().currency.as_deref(), Some("EUR"));
		let t = parse("-$4.50").unwrap();
		assert_eq!(t.currency.as_deref(), Some("USD"));
		assert!(t.value.is_negative());
		assert_eq!(parse("12").unwrap().currency, None);
	}
}
