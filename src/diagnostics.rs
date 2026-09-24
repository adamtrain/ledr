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

//! Errors and warnings that know where in the ledger they came from.
//!
//! A [`Diagnostic`] is an ordinary `std::error::Error`, so it travels through
//! `anyhow` like any other error. Front ends downcast to it to render the
//! offending source lines alongside the message.

use crate::syntax::source::Span;
use std::fmt;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Severity {
	Error,
	Warning,
}

/// A problem found in a ledger, optionally pinned to a place in its source.
#[derive(Clone, Debug)]
pub struct Diagnostic {
	pub severity: Severity,
	pub message: String,
	/// Where the problem is. A span covering several lines (such as a whole
	/// entry) is shown in full.
	pub span: Option<Span>,
	/// A short note printed under the pointed-at source, if any.
	pub label: Option<String>,
	/// Extra lines of explanation.
	pub notes: Vec<String>,
	/// A suggestion for how to fix the problem.
	pub help: Option<String>,
}

impl Diagnostic {
	pub fn error(message: impl Into<String>) -> Self {
		Self::new(Severity::Error, message)
	}

	pub fn warning(message: impl Into<String>) -> Self {
		Self::new(Severity::Warning, message)
	}

	fn new(severity: Severity, message: impl Into<String>) -> Self {
		Self {
			severity,
			message: message.into(),
			span: None,
			label: None,
			notes: vec![],
			help: None,
		}
	}

	/// Pins this to a place in the source, unless it already has one.
	pub fn at(mut self, span: Span) -> Self {
		self.span.get_or_insert(span);
		self
	}

	pub fn at_opt(self, span: Option<Span>) -> Self {
		match span {
			Some(span) => self.at(span),
			None => self,
		}
	}

	pub fn label(mut self, label: impl Into<String>) -> Self {
		self.label = Some(label.into());
		self
	}

	pub fn note(mut self, note: impl Into<String>) -> Self {
		self.notes.push(note.into());
		self
	}

	pub fn help(mut self, help: impl Into<String>) -> Self {
		self.help = Some(help.into());
		self
	}

	/// Converts any error into a diagnostic pinned to the given span. An
	/// error that already is a diagnostic keeps its own details, and only
	/// gains a location if it lacked one.
	pub fn locate(error: anyhow::Error, span: Span) -> anyhow::Error {
		match error.downcast::<Diagnostic>() {
			Ok(diagnostic) => diagnostic.at(span).into(),
			Err(other) => Diagnostic::error(other.to_string()).at(span).into(),
		}
	}
}

impl fmt::Display for Diagnostic {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		write!(f, "{}", self.message)
	}
}

impl std::error::Error for Diagnostic {}

/// Extension for attaching a source location to any fallible result.
pub trait Locate<T> {
	fn locate(self, span: Span) -> Result<T, anyhow::Error>;
}

impl<T> Locate<T> for Result<T, anyhow::Error> {
	fn locate(self, span: Span) -> Result<T, anyhow::Error> {
		self.map_err(|e| Diagnostic::locate(e, span))
	}
}

/// Suggests the candidate closest to `input`, if any is plausibly what was
/// meant. Used for "did you mean" hints on misspelled names.
pub fn closest<'a>(
	input: &str,
	candidates: impl IntoIterator<Item = &'a str>,
) -> Option<&'a str> {
	let input_lower = input.to_lowercase();
	candidates
		.into_iter()
		.filter(|c| *c != input)
		.map(|c| (edit_distance(&input_lower, &c.to_lowercase()), c))
		.filter(|(distance, c)| {
			// Allow roughly one typo per four characters
			*distance
				<= (c.chars().count().max(input.chars().count()) / 4).max(1)
		})
		.min_by_key(|(distance, _)| *distance)
		.map(|(_, c)| c)
}

/// Optimal string alignment distance: insertions, deletions, substitutions
/// and transpositions of adjacent characters each cost one.
pub fn edit_distance(a: &str, b: &str) -> usize {
	let a: Vec<char> = a.chars().collect();
	let b: Vec<char> = b.chars().collect();
	let mut rows = vec![vec![0usize; b.len() + 1]; a.len() + 1];
	for (i, row) in rows.iter_mut().enumerate() {
		row[0] = i;
	}
	for (j, cell) in rows[0].iter_mut().enumerate() {
		*cell = j;
	}
	for i in 1..=a.len() {
		for j in 1..=b.len() {
			let cost = usize::from(a[i - 1] != b[j - 1]);
			let mut best = (rows[i - 1][j] + 1)
				.min(rows[i][j - 1] + 1)
				.min(rows[i - 1][j - 1] + cost);
			if i > 1 && j > 1 && a[i - 1] == b[j - 2] && a[i - 2] == b[j - 1] {
				best = best.min(rows[i - 2][j - 2] + 1);
			}
			rows[i][j] = best;
		}
	}
	rows[a.len()][b.len()]
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn test_edit_distance() {
		assert_eq!(edit_distance("", ""), 0);
		assert_eq!(edit_distance("abc", "abc"), 0);
		assert_eq!(edit_distance("abc", "abd"), 1);
		assert_eq!(edit_distance("abc", "acb"), 1);
		assert_eq!(edit_distance("kitten", "sitting"), 3);
	}

	#[test]
	fn test_closest_finds_typos() {
		let accounts = ["Assets:Checking", "Assets:Savings", "Expenses:Food"];
		assert_eq!(
			closest("Assets:Chekcing", accounts),
			Some("Assets:Checking")
		);
		assert_eq!(closest("assets:savings", accounts), Some("Assets:Savings"));
		assert_eq!(closest("Liabilities:Visa", accounts), None);
	}
}
