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
use crate::gl::entry::Entry;
use crate::reports::table::Table;
use crate::util::date::Date;
use crate::util::quant::Quant;
use std::collections::{BTreeMap, BTreeSet};

/// Every entry that touched a set of accounts, with running balances.
#[derive(Debug)]
pub struct Register {
	/// What the accounts were matched against
	pub pattern: String,
	/// The distinct accounts that matched
	pub accounts: Vec<String>,
	pub rows: Vec<RegisterRow>,
	/// Net movement per description, in order of first appearance
	pub by_description: Vec<DescriptionTotal>,
	/// Net movement over the rows shown, by currency
	pub totals: Vec<(String, Quant)>,
	/// Balance before the first row shown, from any earlier entries
	pub opening: Vec<(String, Quant)>,
	/// Balance after the last row
	pub closing: Vec<(String, Quant)>,
}

#[derive(Debug)]
pub struct RegisterRow {
	pub date: Date,
	pub description: String,
	pub reference: String,
	/// Net effect of the entry on the accounts, by currency
	pub amounts: Vec<(String, Quant)>,
	/// Running balance after this entry, for the same currencies
	pub balances: Vec<(String, Quant)>,
}

#[derive(Debug)]
pub struct DescriptionTotal {
	pub description: String,
	pub amounts: Vec<(String, Quant)>,
	pub count: usize,
}

impl Register {
	/// Builds the register for accounts whose names contain `pattern`. A
	/// currency, if given, filters out movement in all other currencies;
	/// there is no conversion in this report.
	/// Entries before `begin` only count towards the opening balance.
	pub fn new(
		mut entries: Vec<Entry>,
		pattern: &str,
		currency_filter: Option<&str>,
		begin: Date,
	) -> Self {
		entries.sort();

		let mut accounts = BTreeSet::new();
		let mut rows = vec![];
		let mut running: BTreeMap<String, Quant> = BTreeMap::new();
		let mut totals: BTreeMap<String, Quant> = BTreeMap::new();
		let mut opening: Option<BTreeMap<String, Quant>> = None;
		let mut by_description: Vec<DescriptionTotal> = vec![];

		for entry in &entries {
			for detail in entry.details() {
				if detail.account().contains(pattern) {
					accounts.insert(detail.account().clone());
				}
			}

			let amounts: Vec<(String, Quant)> = entry
				.net_for_account(&pattern.to_string())
				.into_iter()
				.filter(|(c, _)| currency_filter.is_none_or(|f| f == c))
				.collect();
			if amounts.is_empty() {
				continue;
			}

			if *entry.get_date() < begin {
				for (currency, value) in &amounts {
					*running.entry(currency.clone()).or_default() += *value;
				}
				continue;
			}
			opening.get_or_insert_with(|| running.clone());

			for (currency, value) in &amounts {
				*totals.entry(currency.clone()).or_default() += *value;
			}
			let mut balances = vec![];
			for (currency, value) in &amounts {
				let balance = running.entry(currency.clone()).or_default();
				*balance += *value;
				balances.push((currency.clone(), *balance));
			}

			let index = match by_description
				.iter()
				.position(|d| d.description == *entry.get_desc())
			{
				Some(i) => i,
				None => {
					by_description.push(DescriptionTotal {
						description: entry.get_desc().clone(),
						amounts: vec![],
						count: 0,
					});
					by_description.len() - 1
				},
			};
			let group = &mut by_description[index];
			group.count += 1;
			for (currency, value) in &amounts {
				match group.amounts.iter_mut().find(|(c, _)| c == currency) {
					Some((_, total)) => *total += *value,
					None => group.amounts.push((currency.clone(), *value)),
				}
			}

			rows.push(RegisterRow {
				date: *entry.get_date(),
				description: entry.get_desc().clone(),
				reference: entry.get_reference(),
				amounts,
				balances,
			});
		}

		for group in &mut by_description {
			group.amounts.sort_by(|a, b| a.0.cmp(&b.0));
		}

		Self {
			pattern: pattern.to_string(),
			accounts: accounts.into_iter().collect(),
			rows,
			by_description,
			totals: totals.into_iter().collect(),
			opening: opening
				.unwrap_or_else(|| running.clone())
				.into_iter()
				.filter(|(_, v)| !v.is_zero())
				.collect(),
			closing: running.into_iter().collect(),
		}
	}

	/// Three sections: individual entries, totals by description, and grand
	/// totals, in a plain table.
	pub fn plain(&self) -> String {
		if self.rows.is_empty() {
			return "No data\n".to_string();
		}

		let mut table = Table::new(5);
		table.right_align(vec![1, 2]);

		for row in &self.rows {
			for (i, (currency, total)) in row.amounts.iter().enumerate() {
				if i == 0 {
					// first row we print full detail
					table.add_row(vec![
						&row.date.to_string(),
						&row.description,
						&total.to_string(),
						currency,
						&row.reference,
					]);
				} else {
					// subsequent rows we only print amounts
					table.add_row(vec![
						"",
						"↪ ",
						&total.to_string(),
						currency,
						"",
					]);
				}
			}
		}

		table.add_partial_separator(vec![1, 2]);

		let mut descriptions: Vec<&DescriptionTotal> =
			self.by_description.iter().collect();
		descriptions.sort_by(|a, b| a.description.cmp(&b.description));
		for group in descriptions {
			for (currency, total) in &group.amounts {
				table.add_row(vec![
					"",
					&group.description,
					&total.to_string(),
					currency,
					"",
				]);
			}
		}

		table.add_partial_separator(vec![2]);

		for (currency, total) in &self.totals {
			table.add_row(vec!["", "", &total.to_string(), currency, ""]);
		}

		table.render()
	}
}
