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
use crate::investment::commodity::Commodity;
use crate::investment::sale::Sale;
use crate::util::date::{Date, Duration};
use crate::util::quant::Quant;
use std::cmp::Ordering;

/// A discrete batch of a commodity that was purchased or acquired in a single
/// transaction. It may be closed or open, and it may be partially closed with
/// associated sales. These may differ in date, amount, and proceeds compared
/// with each other.
///
/// Lots have an ID, by default an integer, that can be used to match them.
/// Otherwise, FIFO is assumed.
#[derive(Debug, PartialEq, Eq)]
pub struct Lot {
	pub id: String,

	/// Indicates whether the user specifically named this lot
	pub is_named: bool,

	pub status: LotStatus,
	pub account: String,

	pub commodity: Commodity,
	pub quantity: Quant, // always in positive terms; can't go negative

	pub acquisition_date: Date,

	pub closed_date: Option<Date>,
	pub sales: Vec<Sale>,
}

#[derive(Debug, PartialEq, Eq)]
pub enum LotStatus {
	Open,
	Closed,
}

impl PartialOrd for LotStatus {
	fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
		Some(self.cmp(other))
	}
}

impl Ord for LotStatus {
	fn cmp(&self, other: &Self) -> Ordering {
		match (self, other) {
			(LotStatus::Open, LotStatus::Closed) => Ordering::Less,
			(LotStatus::Closed, LotStatus::Open) => Ordering::Greater,
			_ => Ordering::Equal,
		}
	}
}

impl Lot {
	/// Reports the amount of time the Lot has been held as of a specific
	/// date. If the lot has been closed prior to the passed date, it will
	/// report the time held before it closed, not the time to present.
	pub fn time_held(&self, as_of: &Date) -> Duration {
		let end = match self.closed_date {
			Some(date) => {
				if as_of < &date {
					as_of
				} else {
					&date.clone()
				}
			},
			None => as_of,
		};
		self.acquisition_date.until(end)
	}
}

impl PartialOrd for Lot {
	fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
		Some(self.cmp(other))
	}
}

impl Ord for Lot {
	fn cmp(&self, other: &Self) -> Ordering {
		// First: Compare by acquisition_date (thus, FIFO)
		let acquisition_cmp =
			self.acquisition_date.cmp(&other.acquisition_date);
		if acquisition_cmp != Ordering::Equal {
			return acquisition_cmp;
		}

		// Then: Compare by status (Open < Closed)
		let status_cmp = self.status.cmp(&other.status);
		if status_cmp != Ordering::Equal {
			return status_cmp;
		}

		// Then: Compare by commodity (lexicographically)
		let commodity_cmp = self.commodity.cmp(&other.commodity);
		if commodity_cmp != Ordering::Equal {
			return commodity_cmp;
		}

		// Then: Compare by quantity (ascending)
		let quantity_cmp = self.quantity.cmp(&other.quantity);
		if quantity_cmp != Ordering::Equal {
			return quantity_cmp;
		}

		// Then: Compare by number of sales (descending)
		let sales_cmp = other.sales.len().cmp(&self.sales.len());
		if sales_cmp != Ordering::Equal {
			return sales_cmp;
		}

		// Then: Compare by account (lexicographically)
		let account_cmp = self.account.cmp(&other.account);
		if account_cmp != Ordering::Equal {
			return account_cmp;
		}

		Ordering::Equal
	}
}
