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

//! Ledger source text: the files that were read, and spans pointing into them.

use std::path::{Path, PathBuf};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct FileId(usize);

/// A location in a source file: part of one line, or a run of whole lines.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Span {
	pub file: FileId,
	/// First line, counted from 1
	pub line: usize,
	/// Last line, inclusive. Equal to `line` for single-line spans.
	pub last_line: usize,
	/// Byte columns within `line`, counted from 0. Only meaningful for
	/// single-line spans, where they mark exactly what is being pointed at.
	pub start: usize,
	pub end: usize,
}

impl Span {
	pub fn new(file: FileId, line: usize, start: usize, end: usize) -> Self {
		Self {
			file,
			line,
			last_line: line,
			start,
			end,
		}
	}

	/// A span over whole lines, from this one's first line through `other`'s
	/// last line.
	pub fn through(self, other: Span) -> Self {
		Self {
			last_line: other.last_line.max(self.line),
			start: 0,
			end: 0,
			..self
		}
	}

	pub fn is_multiline(&self) -> bool {
		self.last_line > self.line
	}
}

#[derive(Debug)]
pub struct SourceFile {
	/// The path as ledr resolved it, used for display
	pub path: PathBuf,
	pub text: String,
	line_starts: Vec<usize>,
}

impl SourceFile {
	pub fn new(path: PathBuf, text: String) -> Self {
		let line_starts = std::iter::once(0)
			.chain(text.match_indices('\n').map(|(i, _)| i + 1))
			.collect();
		Self {
			path,
			text,
			line_starts,
		}
	}

	pub fn line_count(&self) -> usize {
		// A trailing newline does not begin another line
		if self.text.ends_with('\n') {
			self.line_starts.len() - 1
		} else {
			self.line_starts.len()
		}
	}

	/// The text of a line (counted from 1), without its line ending.
	pub fn line(&self, number: usize) -> &str {
		let Some(&start) = self.line_starts.get(number.wrapping_sub(1)) else {
			return "";
		};
		let end = self
			.line_starts
			.get(number)
			.map_or(self.text.len(), |&next| next - 1);
		self.text[start..end.max(start)].trim_end_matches('\r')
	}

	pub fn lines(&self) -> impl Iterator<Item = (usize, &str)> {
		(1..=self.line_count()).map(|n| (n, self.line(n)))
	}
}

/// Every file read while loading a ledger, in the order first read.
#[derive(Debug, Default)]
pub struct SourceMap {
	files: Vec<SourceFile>,
}

impl SourceMap {
	pub fn new() -> Self {
		Self::default()
	}

	pub fn add(&mut self, file: SourceFile) -> FileId {
		self.files.push(file);
		FileId(self.files.len() - 1)
	}

	pub fn get(&self, id: FileId) -> &SourceFile {
		&self.files[id.0]
	}

	pub fn path(&self, id: FileId) -> &Path {
		&self.files[id.0].path
	}

	pub fn files(&self) -> impl Iterator<Item = (FileId, &SourceFile)> {
		self.files.iter().enumerate().map(|(i, f)| (FileId(i), f))
	}

	pub fn len(&self) -> usize {
		self.files.len()
	}

	pub fn is_empty(&self) -> bool {
		self.files.is_empty()
	}

	/// `path:line`, the conventional way to point at a place in a file
	pub fn describe(&self, span: &Span) -> String {
		format!("{}:{}", self.path(span.file).display(), span.line)
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn test_lines() {
		let file = SourceFile::new("x".into(), "a\nbb\r\n\nccc".into());
		assert_eq!(file.line_count(), 4);
		assert_eq!(file.line(1), "a");
		assert_eq!(file.line(2), "bb");
		assert_eq!(file.line(3), "");
		assert_eq!(file.line(4), "ccc");
		assert_eq!(file.line(5), "");
	}

	#[test]
	fn test_trailing_newline_is_not_a_line() {
		let file = SourceFile::new("x".into(), "a\nb\n".into());
		assert_eq!(file.line_count(), 2);
		assert_eq!(file.lines().count(), 2);
	}
}
