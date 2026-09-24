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

//! Settings kept between runs, in `~/.config/ledr/config.toml`: for now,
//! just which ledger to read when none is given.

use crate::diagnostics::Diagnostic;
use crate::syntax::source::{SourceFile, SourceMap, Span};
use anyhow::{Error, anyhow};
use serde::Deserialize;
use std::path::{Path, PathBuf};

#[derive(Debug, Default, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Config {
	/// The ledger to read when neither -f nor LEDR_FILE is given
	pub file: Option<String>,
}

fn env_path(name: &str) -> Option<PathBuf> {
	std::env::var_os(name)
		.map(PathBuf::from)
		.filter(|p| p.is_absolute())
}

/// The person's home directory
pub fn home() -> Option<PathBuf> {
	env_path("HOME").or_else(|| env_path("USERPROFILE"))
}

/// Where the config file lives: under `$XDG_CONFIG_HOME` if that is set,
/// else `~/.config`, as most command line tools do on every platform
pub fn path() -> Option<PathBuf> {
	let base = env_path("XDG_CONFIG_HOME")
		.or_else(|| home().map(|h| h.join(".config")))
		.or_else(|| env_path("APPDATA"))?;
	Some(base.join("ledr").join("config.toml"))
}

/// Expands a leading `~` to the home directory
pub fn expand_home(path: &str) -> PathBuf {
	let rest = match path.strip_prefix('~') {
		Some("") => "",
		Some(rest) if rest.starts_with(['/', '\\']) => &rest[1..],
		_ => return PathBuf::from(path),
	};
	match home() {
		Some(home) => home.join(rest),
		None => PathBuf::from(path),
	}
}

/// A path as a person would write it, with the home directory as `~`,
/// whether or not the home directory was reached through a symbolic link
pub fn display(path: &Path) -> String {
	let homes = home()
		.into_iter()
		.flat_map(|home| [std::fs::canonicalize(&home).ok(), Some(home)])
		.flatten();
	for home in homes {
		if let Ok(rest) = path.strip_prefix(&home) {
			return if rest.as_os_str().is_empty() {
				"~".into()
			} else {
				format!("~/{}", rest.display())
			};
		}
	}
	path.display().to_string()
}

/// The settings, if there is a config file. Errors in it are shown with its
/// text, like errors in a ledger, so `sources` receives it.
pub fn load(sources: &mut SourceMap) -> Result<Option<Config>, Error> {
	let Some(path) = path() else {
		return Ok(None);
	};
	let text = match std::fs::read_to_string(&path) {
		Ok(text) => text,
		Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
		Err(e) => {
			return Err(anyhow!("Could not read `{}`: {e}", display(&path)));
		},
	};
	parse(&text).map(Some).map_err(|(message, range)| {
		let file = sources.add(SourceFile::new(path.clone(), text.clone()));
		let mut error = Diagnostic::error(format!(
			"ledr's config has a mistake: {message}"
		))
		.help("the settings are described under FILES in `man ledr`");
		if let Some(range) = range {
			error = error.at(span_of(&text, file, range));
		}
		error.into()
	})
}

fn parse(
	text: &str,
) -> Result<Config, (String, Option<std::ops::Range<usize>>)> {
	toml::from_str(text).map_err(|e: toml::de::Error| {
		(e.message().trim().to_string(), e.span())
	})
}

/// Points at a byte range, or at its first line if it spans several
fn span_of(
	text: &str,
	file: crate::syntax::source::FileId,
	range: std::ops::Range<usize>,
) -> Span {
	let start = range.start.min(text.len());
	let line_start = text[..start].rfind('\n').map_or(0, |i| i + 1);
	let line_end = text[start..].find('\n').map_or(text.len(), |i| start + i);
	let line = text[..start].matches('\n').count() + 1;
	let end = range.end.min(line_end).max(start + 1);
	Span::new(file, line, start - line_start, end - line_start)
}

/// The ledger named in the config, resolved: `~` is the home directory, and
/// a relative path is relative to the config file, just as an include is
/// relative to the file that includes it
pub fn ledger(config: &Config) -> Option<String> {
	let file = config.file.as_deref()?.trim();
	if file.is_empty() {
		return None;
	}
	let expanded = expand_home(file);
	let resolved = if expanded.is_relative() {
		match path().as_deref().and_then(Path::parent) {
			Some(dir) => dir.join(expanded),
			None => expanded,
		}
	} else {
		expanded
	};
	Some(resolved.to_string_lossy().into_owned())
}

/// Writes `value` as a TOML string: literal if that needs no escapes, as
/// Windows paths often would, else basic
fn toml_string(value: &str) -> String {
	if !value.contains('\'') && !value.chars().any(char::is_control) {
		return format!("'{value}'");
	}
	let mut out = String::from("\"");
	for c in value.chars() {
		match c {
			'"' => out.push_str("\\\""),
			'\\' => out.push_str("\\\\"),
			'\n' => out.push_str("\\n"),
			'\t' => out.push_str("\\t"),
			c if c.is_control() => {
				out.push_str(&format!("\\u{:04X}", c as u32))
			},
			c => out.push(c),
		}
	}
	out.push('"');
	out
}

/// The config text with `file` set to `ledger`, keeping everything else in
/// it as it was, comments included
fn with_file(existing: Option<&str>, ledger: &str) -> String {
	let setting = format!("file = {}", toml_string(ledger));
	let Some(existing) = existing.filter(|t| !t.trim().is_empty()) else {
		return format!(
			"# ledr's settings. See FILES in `man ledr`.\n\n\
			# The ledger to read when neither -f nor LEDR_FILE is given\n\
			{setting}\n"
		);
	};
	let is_setting = |line: &str| {
		line.trim_start()
			.strip_prefix("file")
			.is_some_and(|rest| rest.trim_start().starts_with('='))
	};
	let mut replaced = false;
	let mut out: Vec<String> = existing
		.lines()
		.map(|line| {
			if !replaced && is_setting(line) {
				replaced = true;
				setting.clone()
			} else {
				line.to_string()
			}
		})
		.collect();
	if !replaced {
		out.push(setting);
	}
	out.join("\n") + "\n"
}

/// Remembers `ledger` as the one to read by default, returning where the
/// config file is. A ledger under the home directory is written with `~`,
/// so the config still works if it is shared between computers.
pub fn remember(ledger: &Path) -> Result<PathBuf, Error> {
	let path = path().ok_or_else(|| {
		anyhow!("Could not tell where to keep ledr's config: HOME isn't set")
	})?;
	let existing = match std::fs::read_to_string(&path) {
		Ok(text) => Some(text),
		Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
		Err(e) => {
			return Err(anyhow!("Could not read `{}`: {e}", display(&path)));
		},
	};
	if let Some(text) = &existing
		&& let Err((message, _)) = parse(text)
	{
		return Err(Diagnostic::error(format!(
			"Didn't change `{}`, because it has a mistake: {message}",
			display(&path)
		))
		.help("fix it, or delete it and run `ledr init` again")
		.into());
	}

	let text = with_file(existing.as_deref(), &display(ledger));
	if let Some(dir) = path.parent() {
		std::fs::create_dir_all(dir)
			.map_err(|e| anyhow!("Could not create `{}`: {e}", display(dir)))?;
	}
	std::fs::write(&path, text)
		.map_err(|e| anyhow!("Could not write `{}`: {e}", display(&path)))?;
	Ok(path)
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn test_parse() {
		assert_eq!(parse("").unwrap(), Config::default());
		assert_eq!(
			parse("# hi\nfile = '~/books/main.ledr'\n").unwrap().file,
			Some("~/books/main.ledr".into())
		);
		let (message, range) = parse("fiel = 'x'\n").unwrap_err();
		assert!(message.contains("unknown field `fiel`"), "{message}");
		assert_eq!(range, Some(0..4));
	}

	#[test]
	fn test_strings_survive_a_round_trip() {
		for value in [
			"~/books/main.ledr",
			r"C:\Users\me\books.ledr",
			"it's here",
			"quote \" and 'both' \\",
			"tab\there",
		] {
			let text = with_file(None, value);
			assert_eq!(parse(&text).unwrap().file.as_deref(), Some(value));
		}
	}

	#[test]
	fn test_with_file_keeps_everything_else() {
		let existing = "# mine\nfile = \"old.ledr\" # was here\n\n# end\n";
		assert_eq!(
			with_file(Some(existing), "new.ledr"),
			"# mine\nfile = 'new.ledr'\n\n# end\n"
		);
		assert_eq!(
			with_file(Some("# nothing yet"), "a.ledr"),
			"# nothing yet\nfile = 'a.ledr'\n"
		);
		let fresh = with_file(None, "a.ledr");
		assert!(fresh.starts_with("# ledr's settings"));
		assert!(fresh.ends_with("file = 'a.ledr'\n"));
		// A setting that merely starts with "file" is left alone
		assert_eq!(
			with_file(Some("filed = 1"), "a.ledr"),
			"filed = 1\nfile = 'a.ledr'\n"
		);
	}

	#[test]
	fn test_span_of_points_at_the_line() {
		let text = "# first\nfiel = 'x'\n";
		let mut map = SourceMap::new();
		let file = map.add(SourceFile::new("config".into(), text.into()));
		let span = span_of(text, file, 8..12);
		assert_eq!((span.line, span.start, span.end), (2, 0, 4));
	}
}
