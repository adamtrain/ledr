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
use crate::util::date::Date;
use anyhow::{bail, Error};
use std::collections::BTreeMap;

/// An indication from a user that a given object is or is not active or
/// available for use as of given date.
#[derive(Debug)]
pub struct Declaration {
	events: BTreeMap<Date, bool>, // calendar date -> whether item is active
}

impl Declaration {
	pub fn new() -> Self {
		Self {
			events: BTreeMap::new(),
		}
	}

	pub fn open_account(&mut self, date: Date) -> Result<(), Error> {
		if self.events.contains_key(&date) {
			bail!("Cannot make multiple declarations for the same account on the same date")
		}

		self.events.insert(date, true);
		Ok(())
	}

	pub fn close_account(&mut self, date: Date) -> Result<(), Error> {
		if self.events.contains_key(&date) {
			bail!("Cannot make multiple declarations for the same account on the same date")
		}

		self.events.insert(date, false);
		Ok(())
	}

	/// Returns true if the most recent event on or prior to the given date is
	/// set to open, else false. Defaults to false.
	pub fn is_open_on(&self, date: &Date) -> bool {
		// Find event on or before the given date
		if let Some((_, &status)) = self.events.range(..=date).next_back() {
			return status;
		}
		false
	}
}
