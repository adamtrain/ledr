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
use crate::gl::entry::Entry;
use crate::reports::table::Table;
use crate::util::quant::Quant;
use std::collections::BTreeMap;

pub struct LedgerReporter {
	entries: Vec<Entry>,
}

impl LedgerReporter {
	pub fn new(mut entries: Vec<Entry>) -> Self {
		entries.sort();

		Self { entries }
	}

	/// Reports entries and their net effect on the given account. Will report
	/// all entries unless a currency is provided, in which case sums not in
	/// the given currency will be filtered out. There is no currency
	/// conversion in this report; currency is a simple filter.
	///
	/// Has three sections: individual entries, totals by merchant, and grand
	/// total.
	pub fn account_summary(
		&self,
		account: &String,
		currency_filter: Option<String>,
	) {
		if self.entries.is_empty() {
			println!("No data");
			return;
		}

		let mut table = Table::new(5);
		table.right_align(vec![1, 2]);

		let mut totals = BTreeMap::new();

		// desc -> map of currency to total net movement
		let mut totals_by_desc = BTreeMap::new();

		for entry in &self.entries {
			let net_map = entry.net_for_account(account);
			for (i, (currency, total)) in net_map.iter().enumerate() {
				if currency_filter.is_some()
					&& currency_filter.as_ref().unwrap() != currency
				{
					continue;
				}

				*totals_by_desc
					.entry(entry.get_desc().clone())
					.or_insert_with(BTreeMap::new)
					.entry(currency.clone())
					.or_insert(Quant::zero()) += *total;

				if i == 0 {
					// first row we print full detail
					table.add_row(vec![
						&entry.get_date().to_string(),
						entry.get_desc(),
						&total.to_string(),
						&currency.to_string(),
						&entry.get_reference(),
					]);
				} else {
					// subsequent rows we only print amounts
					table.add_row(vec![
						&"",
						&"↪ ",
						&total.to_string(),
						&currency.to_string(),
						&"",
					]);
				}

				*totals.entry(currency.clone()).or_insert(Quant::zero()) +=
					*total;
			}
		}

		table.add_partial_separator(vec![1, 2]);

		for (desc, m) in &totals_by_desc {
			for (currency, total) in m {
				table.add_row(vec!["", desc, &total.to_string(), currency, ""]);
			}
		}

		table.add_partial_separator(vec![2]);

		for (currency, total) in totals {
			table.add_row(vec!["", "", &total.to_string(), &currency, ""]);
		}

		table.print();
	}
}
