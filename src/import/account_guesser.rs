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
use crate::gl::ledger::Ledger;
use std::collections::HashMap;

/// Constructed from a Ledger. Assembles a lookup map from Entry descriptions
/// to counterparty accounts, with the goal of prefilling those imported
/// transactions with exact description matches (i.e. transactions with
/// merchants that have occurred before) with the accounts previously used.
///
/// If multiple accounts have been used with the same description, defaults to
/// the most recent.
pub struct AccountGuesser {
	lookup: HashMap<String, String>, // description -> account name
}

impl AccountGuesser {
	pub fn new(ledger: Ledger) -> AccountGuesser {
		let mut lookup = HashMap::new();

		for entry in ledger.take_entries() {
			if let Some(cpd) = entry.get_counterparty_detail() {
				lookup.insert(entry.get_desc().clone(), cpd.account().clone());
			}
		}

		Self { lookup }
	}

	pub fn lookup(&self, desc: &str) -> Option<String> {
		self.lookup.get(desc).cloned()
	}
}
