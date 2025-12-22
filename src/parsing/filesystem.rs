/* Copyright © 2024-2026 Adam Train <adam@usdocument.org>
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
use anyhow::{bail, Error};
use std::collections::HashSet;
use std::fs::File;
use std::path::Path;

pub struct Filesystem {
	/// Set of file paths that have been inspected.
	/// Used to avoid circular includes.
	included_files: HashSet<String>,
}

impl Filesystem {
	pub fn new() -> Self {
		Self {
			included_files: HashSet::new(),
		}
	}

	pub fn open(&self, file_path: &str) -> Result<File, Error> {
		let path = Path::new(file_path);
		let file = File::open(path)?;
		Ok(file)
	}

	pub fn declare_file(&mut self, file_path: &str) -> Result<(), Error> {
		if self.included_files.contains(file_path) {
			bail!("Circular file includes: {file_path}")
		}
		self.included_files.insert(file_path.parse()?);
		Ok(())
	}

	/// Resolves a path relative to the directory of the parent file.
	/// If the path is absolute, it's returned as is.
	pub fn resolve_path(
		&self,
		parent_file_path: &str,
		relative_path: &str,
	) -> String {
		let path = Path::new(relative_path);
		if path.is_absolute() {
			return relative_path.to_string();
		}

		let parent_dir = Path::new(parent_file_path)
			.parent()
			.unwrap_or(Path::new(""));

		parent_dir
			.join(path)
			.to_str()
			.unwrap_or(relative_path)
			.to_string()
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn test_declare_file() {
		let mut filesystem = Filesystem::new();
		assert!(filesystem.declare_file("path/to/file").is_ok());
		assert!(filesystem.included_files.contains("path/to/file"));
		assert!(filesystem.declare_file("path/to/file").is_err());
	}
}
