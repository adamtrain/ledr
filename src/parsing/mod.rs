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

//! Turning ledger files into a finished [`Ledger`].

pub mod loader;
pub mod parser;

use crate::gl::ledger::Ledger;
use crate::investment::portfolio::Portfolio;
use crate::syntax::source::SourceMap;
use crate::util::date::Date;
use anyhow::Error;
use loader::Overlay;
use parser::ParseResult;

/// Everything that decides how a ledger is read.
#[derive(Clone, Debug)]
pub struct LoadOptions {
	/// The root ledger file, or `-` for stdin
	pub file: String,
	/// Skip checks that accounts and currencies are declared
	pub lenient: bool,
	/// Run expensive consistency checks too
	pub thorough: bool,
	/// Collect problems with declarations and entries instead of stopping at
	/// the first, e.g. to report them all at once
	pub keep_going: bool,
	/// Drop entries before this date from reports
	pub begin: Date,
	/// Ignore everything after this date entirely
	pub end: Date,
	/// Cap on decimal places shown for any currency
	pub precision: Option<u32>,
	/// Text to treat as appended to one of the files
	pub overlay: Option<Overlay>,
}

impl LoadOptions {
	pub fn new(file: impl Into<String>) -> Self {
		Self {
			file: file.into(),
			lenient: false,
			thorough: false,
			keep_going: false,
			begin: Date::min(),
			end: Date::max(),
			precision: None,
			overlay: None,
		}
	}
}

/// A ledger, fully read, balanced and checked.
#[derive(Debug)]
pub struct Loaded {
	pub ledger: Ledger,
	pub result: ParseResult,
	/// The state of every lot, bought and sold
	pub portfolio: Portfolio,
	/// Every line read, in include order
	pub items: Vec<loader::Item>,
}

/// Reads, builds and finalizes a ledger. Source text is kept in `sources`,
/// which the caller owns so that it can show where any error came from.
pub fn load_ledger(
	options: &LoadOptions,
	sources: &mut SourceMap,
) -> Result<Loaded, Error> {
	let items = loader::load(&options.file, sources, options.overlay.as_ref())?;

	let mut ledger = Ledger::new(options.lenient, options.thorough);
	let result =
		parser::build(&items, &mut ledger, &options.end, options.keep_going)?;

	let rate_warnings = ledger
		.exchange_rates
		.finalize(&result.max_precision_by_currency)?;
	ledger.warnings.extend(rate_warnings);

	let portfolio = ledger.lots.tabulate()?;

	ledger.finalize(
		&options.begin,
		&result.max_precision_by_currency,
		options.precision,
	)?;

	Ok(Loaded {
		ledger,
		result,
		portfolio,
		items,
	})
}
