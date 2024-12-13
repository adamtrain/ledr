/* Copyright © 2024-2025 Adam Train <adam@adamtrain.net>
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
use crate::config::config_file::Config;
use anyhow::{anyhow, bail, Error};
use dirs::home_dir;
use std::collections::HashSet;
use std::fs;
use std::fs::File;
use std::path::{Path, PathBuf};
use std::process::Command;

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
			bail!("Circular file includes: {}", file_path)
		}
		self.included_files.insert(file_path.parse()?);
		Ok(())
	}

	/// Fetches the config from the given path, or default path if none.
	/// The boolean argument indicates whether it is necessary to inspect
	/// the config for authentication, i.e. for importing data via API.
	pub fn get_config(
		&self,
		custom_config_path: Option<&String>,
		expand_auth: bool,
	) -> Result<Config, Error> {
		let config_path = match &custom_config_path {
			None => {
				let home_dir = home_dir().unwrap_or_else(|| {
					panic!("Unable to determine home directory")
				});
				home_dir.join(".config/ledr/config.toml")
			},
			Some(p) => PathBuf::from(p),
		};

		// create empty config file if it doesn't exist
		if !config_path.exists() && custom_config_path.is_none() {
			if let Some(parent) = config_path.parent() {
				fs::create_dir_all(parent)?;
			}
			File::create(config_path.clone())?;
		}

		let content = fs::read_to_string(config_path)?;
		let mut config: Config = toml::from_str(&content)
			.map_err(|e| anyhow!("failed to parse config: {}", e))?;

		// Execute api_key_cmd if applicable, and put result in api_key
		if !expand_auth {
			return Ok(config);
		}

		if let Some(imports) = &mut config.imports {
			if let Some(mercury) = &mut imports.mercury {
				if mercury.api_key_cmd.is_some() && mercury.api_key.is_some() {
					bail!("Only one of imports.mercury.api_key and imports.mercury.api_key_cmd may be specified")
				}

				if let Some(api_key_cmd) = &mercury.api_key_cmd {
					let output = Command::new("sh")
						.arg("-c")
						.arg(api_key_cmd)
						.output()
						.map_err(|e| {
							anyhow!("failed to execute api_key_cmd: {}", e)
						})?;

					if output.status.success() {
						mercury.api_key =
							Some(
								String::from_utf8(output.stdout)
									.map_err(|e| {
										anyhow!("failed to parse command output: {}", e)
									})?
									.trim()
									.to_string(),
							);
					} else {
						bail!(
							"mercury api_key_cmd failed with status {}: {}",
							output.status,
							String::from_utf8_lossy(&output.stderr)
						);
					}
				}
			}
		}

		Ok(config)
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
