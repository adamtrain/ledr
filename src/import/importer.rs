/* Copyright © 2024-2025 Adam Train <adam@trainrelay.net>
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
use crate::gl::ledger::Ledger;
use crate::import::mercury::core::MercuryImporter;
use crate::util::date::Date;
use crate::Cli;
use anyhow::{bail, Error};

pub const PLACEHOLDER: &str = "Equity:PLACEHOLDER";

/// The entry point for all import functionality. Currently, the only import
/// target is Mercury, the financial services firm based in the United States.
pub fn import(config: Config, args: Cli, ledger: Ledger) -> Result<(), Error> {
	if args.begin.is_none() || args.end.is_none() {
		bail!("Import requires begin and end dates");
	} else if args.term.is_none() {
		bail!("Please specify the system to import from");
	}

	let mercury_config = config
		.imports
		.unwrap_or_default()
		.mercury
		.unwrap_or_default();

	let (b, e, target) = (
		Date::from_str(&args.begin.unwrap())?,
		Date::from_str(&args.end.unwrap())?,
		args.term.unwrap(),
	);

	let import_target = ImportTarget::from_str(&target)?;
	match import_target {
		ImportTarget::Mercury => {
			let mercury = MercuryImporter::new(mercury_config, ledger)?;
			mercury.run(b, e, args.file)?
		},
	};

	Ok(())
}

pub enum ImportTarget {
	/// https://mercury.com/
	Mercury,
}

impl ImportTarget {
	pub fn from_str(s: &str) -> Result<Self, Error> {
		match s {
			"mercury" => Ok(ImportTarget::Mercury),
			_ => bail!("unknown importer target: {}", s),
		}
	}
}
