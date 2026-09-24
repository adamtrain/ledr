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

//! Reads a ledger file and everything it includes, in include order.

use crate::diagnostics::Diagnostic;
use crate::syntax::lexer::{Line, LineKind, lex_line};
use crate::syntax::source::{FileId, SourceFile, SourceMap, Span};
use anyhow::Error;
use std::fmt;
use std::io::Read;
use std::path::{Path, PathBuf};

/// Stop collecting syntax errors after this many; the rest are noise.
const MAX_ERRORS: usize = 25;

/// One step through the ledger as if every include were pasted in place.
#[derive(Debug)]
pub enum Item {
	Line {
		file: FileId,
		number: usize,
		line: Line,
	},
	/// The end of a file. Like a blank line, it ends any entry in progress.
	EndOfFile(FileId),
}

impl Item {
	/// The whole-line span of a line item
	pub fn span(&self) -> Option<Span> {
		match self {
			Item::Line { file, number, .. } => {
				Some(Span::new(*file, *number, 0, 0))
			},
			Item::EndOfFile(_) => None,
		}
	}
}

/// Text to treat as though it were appended to a file, so that a new entry
/// can be checked in full context before it is written anywhere.
#[derive(Clone, Debug)]
pub struct Overlay {
	pub path: PathBuf,
	pub appended: String,
}

/// Several diagnostics at once, such as every syntax error in a file.
#[derive(Debug)]
pub struct Diagnostics(pub Vec<Diagnostic>);

impl fmt::Display for Diagnostics {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		let messages: Vec<&str> =
			self.0.iter().map(|d| d.message.as_str()).collect();
		write!(f, "{}", messages.join("; "))
	}
}

impl std::error::Error for Diagnostics {}

/// Reads `root` (or stdin, for `-`) and all files it includes into
/// `sources`, returning their lines in include order. Syntax errors are
/// collected across all files and reported together.
pub fn load(
	root: &str,
	sources: &mut SourceMap,
	overlay: Option<&Overlay>,
) -> Result<Vec<Item>, Error> {
	let mut loader = Loader {
		sources,
		overlay,
		stack: vec![],
		seen: vec![],
		items: vec![],
		errors: vec![],
		overlaid: false,
	};

	let path = PathBuf::from(root);
	if root == "-" {
		let mut text = String::new();
		std::io::stdin().read_to_string(&mut text)?;
		loader.add(PathBuf::from("<stdin>"), None, text, None)?;
	} else {
		loader.file(&path, None)?;
	}

	if let Some(overlay) = overlay
		&& !loader.overlaid
		&& loader.errors.is_empty()
	{
		return Err(Diagnostic::error(format!(
			"`{}` isn't part of this ledger",
			overlay.path.display()
		))
		.help(format!(
			"include it from {} first, or add to a file the ledger already reads",
			path.display()
		))
		.into());
	}

	if loader.errors.is_empty() {
		Ok(loader.items)
	} else {
		Err(Diagnostics(loader.errors).into())
	}
}

/// Resolves an included path relative to the directory of the file that
/// includes it. Absolute paths are left alone.
pub fn resolve(including_file: &Path, included: &str) -> PathBuf {
	let included = Path::new(included);
	if included.is_absolute() {
		return included.to_path_buf();
	}
	including_file
		.parent()
		.unwrap_or(Path::new(""))
		.join(included)
}

struct Loader<'a> {
	sources: &'a mut SourceMap,
	overlay: Option<&'a Overlay>,
	/// Canonical paths of the files currently being read, outermost first
	stack: Vec<PathBuf>,
	/// Canonical paths of every file read so far, with where it was included
	seen: Vec<(PathBuf, Option<Span>)>,
	items: Vec<Item>,
	errors: Vec<Diagnostic>,
	/// Whether the overlay found its file
	overlaid: bool,
}

impl Loader<'_> {
	fn file(
		&mut self,
		path: &Path,
		included_at: Option<Span>,
	) -> Result<(), Error> {
		let unreadable = |e: std::io::Error| {
			let error = Diagnostic::error(format!(
				"Could not read `{}`: {}",
				path.display(),
				io_reason(&e)
			))
			.at_opt(included_at);
			match included_at {
				Some(_) => error
					.help("included paths are relative to the including file"),
				None => error,
			}
		};

		let canonical = std::fs::canonicalize(path).map_err(unreadable)?;

		if let Some(depth) = self.stack.iter().position(|p| *p == canonical) {
			let chain: Vec<String> = self.stack[depth..]
				.iter()
				.chain(std::iter::once(&canonical))
				.map(|p| display_name(p))
				.collect();
			return Err(Diagnostic::error("Circular include")
				.at_opt(included_at)
				.note(chain.join(" → "))
				.into());
		}
		if let Some((_, first)) =
			self.seen.iter().find(|(p, _)| *p == canonical)
		{
			let mut error = Diagnostic::error(format!(
				"`{}` is included more than once",
				path.display()
			))
			.at_opt(included_at)
			.help(
				"each file may only be included once, or its entries would count twice",
			);
			if let Some(first) = first {
				error = error.note(format!(
					"first included at {}",
					self.sources.describe(first)
				));
			}
			return Err(error.into());
		}

		let mut text = std::fs::read_to_string(path).map_err(unreadable)?;
		if let Some(overlay) = self.overlay
			&& std::fs::canonicalize(&overlay.path)
				.is_ok_and(|p| p == canonical)
		{
			// Exactly what would be written, so line numbers match
			text.push_str(&overlay.appended);
			self.overlaid = true;
		}

		self.add(path.to_path_buf(), Some(canonical), text, included_at)
	}

	fn add(
		&mut self,
		path: PathBuf,
		canonical: Option<PathBuf>,
		text: String,
		included_at: Option<Span>,
	) -> Result<(), Error> {
		let file = self.sources.add(SourceFile::new(path.clone(), text));
		if let Some(canonical) = &canonical {
			self.stack.push(canonical.clone());
			self.seen.push((canonical.clone(), included_at));
		}

		let lines: Vec<(usize, String)> = self
			.sources
			.get(file)
			.lines()
			.map(|(n, l)| (n, l.to_string()))
			.collect();

		for (number, raw) in lines {
			let line = match lex_line(&raw, file, number) {
				Ok(line) => line,
				Err(error) => {
					self.errors.push(error);
					if self.errors.len() >= MAX_ERRORS {
						return Err(Diagnostics(std::mem::take(
							&mut self.errors,
						))
						.into());
					}
					continue;
				},
			};

			if let LineKind::Include(include) = &line.kind {
				let at = Span::new(
					file,
					number,
					include.cols.start,
					include.cols.end,
				);
				let target = resolve(&path, &include.path);
				self.items.push(Item::Line { file, number, line });
				self.file(&target, Some(at))?;
				continue;
			}

			self.items.push(Item::Line { file, number, line });
		}

		self.items.push(Item::EndOfFile(file));
		if canonical.is_some() {
			self.stack.pop();
		}
		Ok(())
	}
}

fn display_name(path: &Path) -> String {
	path.file_name().map_or_else(
		|| path.display().to_string(),
		|n| n.to_string_lossy().into_owned(),
	)
}

/// The reason part of an I/O error, without Rust's "(os error N)" suffix
fn io_reason(e: &std::io::Error) -> String {
	match e.kind() {
		std::io::ErrorKind::NotFound => "no such file".into(),
		std::io::ErrorKind::PermissionDenied => "permission denied".into(),
		std::io::ErrorKind::IsADirectory => "it is a directory".into(),
		_ => {
			let text = e.to_string();
			match text.find(" (os error") {
				Some(i) => text[..i].to_lowercase(),
				None => text,
			}
		},
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use std::fs;

	fn write(dir: &Path, name: &str, text: &str) -> PathBuf {
		let path = dir.join(name);
		if let Some(parent) = path.parent() {
			fs::create_dir_all(parent).unwrap();
		}
		fs::write(&path, text).unwrap();
		path
	}

	#[test]
	fn test_includes_are_relative_to_including_file() {
		let dir = tempfile::tempdir().unwrap();
		let root = write(dir.path(), "main.ledr", "include sub/a.ledr\n");
		write(dir.path(), "sub/a.ledr", "include b.ledr\n");
		write(dir.path(), "sub/b.ledr", "# leaf\n");

		let mut sources = SourceMap::new();
		let items = load(root.to_str().unwrap(), &mut sources, None).unwrap();
		assert_eq!(sources.len(), 3);
		let ends = items
			.iter()
			.filter(|i| matches!(i, Item::EndOfFile(_)))
			.count();
		assert_eq!(ends, 3);
	}

	#[test]
	fn test_circular_include_names_the_chain() {
		let dir = tempfile::tempdir().unwrap();
		let root = write(dir.path(), "a.ledr", "include b.ledr\n");
		write(dir.path(), "b.ledr", "include a.ledr\n");

		let mut sources = SourceMap::new();
		let error =
			load(root.to_str().unwrap(), &mut sources, None).unwrap_err();
		let diagnostic = error.downcast::<Diagnostic>().unwrap();
		assert_eq!(diagnostic.message, "Circular include");
		assert_eq!(diagnostic.notes, vec!["a.ledr → b.ledr → a.ledr"]);
		assert_eq!(diagnostic.span.unwrap().line, 1);
	}

	#[test]
	fn test_diamond_include_is_reported_as_duplicate() {
		let dir = tempfile::tempdir().unwrap();
		let root =
			write(dir.path(), "a.ledr", "include b.ledr\ninclude c.ledr\n");
		write(dir.path(), "b.ledr", "include d.ledr\n");
		write(dir.path(), "c.ledr", "include d.ledr\n");
		write(dir.path(), "d.ledr", "\n");

		let mut sources = SourceMap::new();
		let error =
			load(root.to_str().unwrap(), &mut sources, None).unwrap_err();
		let diagnostic = error.downcast::<Diagnostic>().unwrap();
		assert!(diagnostic.message.contains("more than once"));
	}

	#[test]
	fn test_missing_file_names_the_file() {
		let mut sources = SourceMap::new();
		let error =
			load("/definitely/not/here.ledr", &mut sources, None).unwrap_err();
		assert!(error.to_string().contains("/definitely/not/here.ledr"));
		assert!(error.to_string().contains("no such file"));
	}

	#[test]
	fn test_collects_every_syntax_error() {
		let dir = tempfile::tempdir().unwrap();
		let root = write(
			dir.path(),
			"a.ledr",
			"2024-13-01 x\n\n! 2024-01-01 acount A:B\n",
		);
		let mut sources = SourceMap::new();
		let error =
			load(root.to_str().unwrap(), &mut sources, None).unwrap_err();
		let all = error.downcast::<Diagnostics>().unwrap();
		assert_eq!(all.0.len(), 2);
	}

	#[test]
	fn test_overlay_is_appended() {
		let dir = tempfile::tempdir().unwrap();
		let root = write(dir.path(), "a.ledr", "# no newline at end");
		let overlay = Overlay {
			path: root.clone(),
			appended: "\n\n2024-01-01 New\n".into(),
		};
		let mut sources = SourceMap::new();
		load(root.to_str().unwrap(), &mut sources, Some(&overlay)).unwrap();
		let (_, file) = sources.files().next().unwrap();
		assert_eq!(file.text, "# no newline at end\n\n2024-01-01 New\n");
	}

	#[test]
	fn test_overlay_must_land_in_the_ledger() {
		let dir = tempfile::tempdir().unwrap();
		let root = write(dir.path(), "a.ledr", "\n");
		let other = write(dir.path(), "notes.txt", "\n");
		let overlay = Overlay {
			path: other,
			appended: "2024-01-01 New\n".into(),
		};
		let mut sources = SourceMap::new();
		let error = load(root.to_str().unwrap(), &mut sources, Some(&overlay))
			.unwrap_err();
		assert!(
			error.to_string().contains("isn't part of this ledger"),
			"{error}"
		);
	}
}
