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
use crate::gl::ledger::Ledger;
use crate::gl::observed_rate::ObservationType;
use crate::parsing::filesystem::Filesystem;
use crate::util::amount::Amount;
use crate::util::date::Date;
use crate::util::quant::Quant;
use anyhow::{anyhow, bail, Error};
use regex::Regex;
use std::collections::{BTreeMap, VecDeque};
use std::fs::File;
use std::io;
use std::io::{BufRead, Seek};

pub struct Parser {
	fs: Filesystem,
	detail_regex: Regex,

	/// Kept to perform sorting of entries on some reports in the order
	/// they appear, second only to their date in sort order.
	entry_count: usize,
}

impl Parser {
	pub fn new() -> Self {
		let re = Regex::new(r#""([^"]*)"|(\S+)"#).unwrap();
		Self {
			detail_regex: re,
			fs: Filesystem::new(),
			entry_count: 0,
		}
	}

	/// Opens and parses the file at file_path into the passed Ledger. We make two
	/// passes through the file: the first processes directives, and the second
	/// processes everything else. This means we are agnostic to the order of any
	/// contents of the file. The only exception is that when multiple implicit
	/// currency conversions occur in the same day between the same currencies, all
	/// reporting will use the latest one processed in the file.
	pub fn parse(
		&mut self,
		file_path: &str,
		ledger: &mut Ledger,
		ignore_after: &Date,
	) -> Result<ParseResult, Error> {
		let mut file = self.fs.open(file_path)?;

		self.first_pass(file_path, &file, ledger, ignore_after)?;
		file.rewind()?;

		// Second pass is responsible for assembling the ParseResult object,
		// which we pass in this way so it can be passed recursively within.
		let mut output: ParseResult = Default::default();
		self.second_pass(&file, ledger, &mut output, ignore_after)?;

		Ok(output)
	}

	/// First pass to process only directive lines. Include statements in the file
	/// may cause this to be called recursively, so it uses the passed Ledger struct
	/// to keep track of files it's traversed before, and block circular inclusion.
	///
	/// Note: The first pass ignores beginning and end bounds, to make sure all
	/// directives are captured; it'd be too annoying to move account and currency
	/// declarations for the interval you want.
	fn first_pass(
		&mut self,
		path: &str,
		file: &File,
		ledger: &mut Ledger,
		ignore_after: &Date,
	) -> Result<(), Error> {
		self.fs.declare_file(path)?;

		let reader = io::BufReader::new(file);

		for (i, line) in reader.lines().enumerate() {
			// Chop comments out
			let l = line?
				.trim()
				.split('#')
				.next()
				.unwrap_or_default()
				.trim()
				.to_string();

			// Skip blank lines
			if l.is_empty() {
				continue;
			}

			// Handle includes, which recursively first_passes when seen
			if l.starts_with("include") {
				let include: Vec<&str> = l.split_whitespace().collect();
				if include.len() != 2 {
					bail!("Invalid include (line {})", i)
				}

				let file = self.fs.open(include[1])?;
				self.first_pass(include[1], &file, ledger, ignore_after)?;
				continue;
			}

			let mut directive: VecDeque<&str> = match l.strip_prefix("!") {
				None => continue,
				Some(d) => d.split_whitespace().collect(),
			};

			if directive.len() < 2 {
				bail!("Invalid directive (line {}): {}", i + 1, l);
			}

			let date_str = directive.pop_front().unwrap();
			let date = Date::from_str(date_str.trim())
				.map_err(|e| anyhow!("{} (line {})", e, i))?;

			if &date > ignore_after {
				continue;
			}

			match directive[0] {
				"account" if directive.len() == 2 => {
					let account = directive[1].to_string();
					if !account.contains(":") {
						bail!("Top level accounts cannot be used on their own (line {})", i);
					}
					ledger
						.declare_account(account, date)
						.map_err(|e| anyhow!("{} (line {})", e, i))?;
				},
				"open" if directive.len() == 2 => {
					let account = directive[1].to_string();
					if !account.contains(":") {
						bail!("Top level accounts cannot be used on their own (line {})", i);
					}
					ledger
						.declare_account_open(account, date)
						.map_err(|e| anyhow!("{} (line {})", e, i))?;
				},
				"close" if directive.len() == 2 => {
					let account = directive[1].to_string();
					if !account.contains(":") {
						bail!("Top level accounts cannot be used on their own (line {})", i);
					}
					ledger
						.declare_account_closure(account, date)
						.map_err(|e| anyhow!("{} (line {})", e, i))?;
				},
				"clear" if directive.len() == 2 => {
					let currency = directive[1].to_string();
					ledger.declare_clear(currency, date);
				},
				"currency" if directive.len() == 2 => {
					let currency = directive[1].to_string();
					ledger
						.declare_currency(&currency, date)
						.map_err(|e| anyhow!("{} (line {})", e, i))?;
				},
				"rate" if directive.len() == 4 => {
					let from = directive[1].to_string();
					let to = directive[2].to_string();
					let rate = directive[3];
					ledger
						.exchange_rates
						.add_rate(
							date,
							from,
							to,
							Quant::from_str(rate)
								.map_err(|e| anyhow!("{} (line {})", e, i))?,
							ObservationType::Declared,
						)
						.map_err(|e| anyhow!("{} (line {})", e, i))?;
				},
				"worthless" if directive.len() == 2 => {
					let currency = directive[1].to_string();
					ledger.exchange_rates.declare_worthless(currency);
				},
				_ => bail!("Invalid directive (line {}): {}", i + 1, l),
			}
		}

		Ok(())
	}

	/// Second pass to process everything else other than directives. Include
	/// statements may cause this method to call itself recursively, but it does
	/// not need to keep track of where it is to avoid circular include statements
	/// because first_pass has already done that.
	fn second_pass(
		&mut self,
		file: &File,
		ledger: &mut Ledger,
		parse_result: &mut ParseResult,
		ignore_after: &Date,
	) -> Result<(), Error> {
		let reader = io::BufReader::new(file);

		let mut ignore_until_next_entry = false;

		for (i, line) in reader.lines().enumerate() {
			// Chop comments out and remove all commas regardless of position
			let l = line?
				.trim()
				.split('#')
				.next()
				.unwrap_or_default()
				.replace(',', "")
				.trim()
				.to_string();

			// If a line is blank, this entry is over (or we are not in one)
			if l.is_empty() {
				ledger
					.finish_entry()
					.map_err(|e| anyhow!("{} (line {})", e, i))?;
				continue;
			}

			// Handle includes, which recursively second_passes when seen.
			// No need to check the structure of the include because the
			// first pass would've failed by now if it were invalid.
			if l.starts_with("include") {
				let include: Vec<&str> = l.split_whitespace().collect();

				let file = self.fs.open(include[1])?;
				self.second_pass(&file, ledger, parse_result, ignore_after)?;
				continue;
			}

			// ignore directives
			if l.starts_with('!') {
				continue;
			}

			// Lines that start with two slashes are reference lines.
			// Empty references are fine; they just do nothing
			if l.starts_with("//") && l.len() > 2 && !ignore_until_next_entry {
				let content = l[2..].trim();
				if !content.is_empty() {
					ledger.add_reference(content.to_string())?;
				}
				continue;
			}

			// Handle entry declaration lines with a date and description
			if let Some((date_str, desc)) = l.split_once(' ') {
				if let Ok(date) = Date::from_str(date_str.trim()) {
					if &date > ignore_after {
						ignore_until_next_entry = true;
						continue;
					}

					ignore_until_next_entry = false;

					ledger
						.new_entry(
							date,
							desc.trim().to_string(),
							self.entry_count,
						)
						.map_err(|e| anyhow!("{} (line {})", e, i))?;

					self.entry_count += 1;

					parse_result.note_date(date);
					continue;
				}
			}

			// Make sure the line is not a date by itself
			if Date::from_str(&l).is_ok() {
				bail!("Orphaned date (line {}): {}", i, l);
			}

			if ignore_until_next_entry {
				continue;
			}

			// Handle entry detail lines, which all have different numbers of
			// terms; with the below regex we split all by whitespace except
			// terms surrounded by quotations, which are for lot naming
			let parts = self.parse_entry_detail(&l);
			if parts.len() == 1 {
				let account = parts[0].clone();
				ledger
					.set_virtual_detail(account)
					.map_err(|e| anyhow!("{} (line {})", e, i))?;
				continue;
			}

			let account = parts[0].to_string();
			let amount = Amount::new(Quant::from_str(&parts[1])?, &parts[2]);
			parse_result.note_precision(
				&amount.currency,
				amount.value.render_precision(),
			);

			match parts.len() {
				// no inline conversion
				3 => ledger
					.add_detail(account, amount, None, None, None)
					.map_err(|e| anyhow!("{} (line {})", e, i))?,
				6 => {
					// inline conversion, i.e. `@ 20.00 USD`
					let is_total_cost = match parts[3].as_str() {
						"@" => false,
						"@@" => true,
						_ => bail!("Invalid format (line {})", i),
					};

					let mut ic_amount = Quant::from_str(parts[4].as_str())
						.map_err(|_| anyhow!("Invalid value (line {})", i))?;
					let ic_currency = parts[5].to_string();

					parse_result.note_precision(
						&ic_currency,
						ic_amount.render_precision(),
					);

					if is_total_cost {
						ic_amount /= amount.value
					};

					ledger
						.add_detail(
							account,
							amount,
							Some(Amount::new(ic_amount, &ic_currency)),
							None,
							None,
						)
						.map_err(|e| anyhow!("{} (line {})", e, i))?
				},
				7 | 8 => {
					// lot declaration, i.e. `{ 20.00 USD }`
					if parts[3] != "{" || parts.last().unwrap() != "}" {
						bail!("Invalid format (line {})", i);
					}

					// Grab cost basis
					let cb_amount = Quant::from_str(parts[4].as_str())
						.map_err(|_| anyhow!("Invalid value (line {})", i))?;
					let cb_currency = parts[5].to_string();

					parse_result.note_precision(
						&cb_currency,
						cb_amount.render_precision(),
					);

					let lot_name = if parts.len() == 8 {
						Some(parts[6].to_string())
					} else {
						None
					};

					// The purchase of a lot implies an exchange rate for that
					// lot on that date, with its cost basis. The sale of a
					// lot does not.
					let implied_conversion = if amount.value > 0 {
						Some(Amount::new(cb_amount, &cb_currency))
					} else {
						None
					};

					ledger
						.add_detail(
							account,
							amount,
							implied_conversion,
							Some(Amount {
								value: cb_amount,
								currency: cb_currency,
							}),
							lot_name,
						)
						.map_err(|e| anyhow!("{} (line {})", e, i))?
				},
				_ => bail!("Invalid format (line {})", i),
			}
		}

		// Make sure to finish the last entry if the file ends without an empty line
		ledger
			.finish_entry()
			.map_err(|e| anyhow!("{} (line eof)", e))?;

		Ok(())
	}

	fn parse_entry_detail(&self, input: &str) -> Vec<String> {
		self.detail_regex
			.captures_iter(input)
			.map(|cap| {
				// Capture either the quoted group or the unquoted group
				cap.get(1).map_or_else(
					move || cap[2].to_string(),
					|m| m.as_str().to_string(),
				)
			})
			.collect()
	}
}

#[derive(Debug, Default)]
pub struct ParseResult {
	/// Greatest amount of precision indicated for each currency
	pub max_precision_by_currency: BTreeMap<String, u32>,
	/// Latest date of any entry (ignores directives)
	pub latest_date: Date,
}

impl ParseResult {
	fn note_precision(&mut self, currency: &str, precision: u32) {
		let entry = self
			.max_precision_by_currency
			.entry(currency.to_owned())
			.or_insert(0);
		*entry = (*entry).max(precision);
	}

	fn note_date(&mut self, date: Date) {
		if self.latest_date < date {
			self.latest_date = date;
		}
	}
}
