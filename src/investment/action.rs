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
use crate::util::amount::Amount;
use crate::util::date::Date;
use crate::util::quant::Quant;
use anyhow::{bail, Error};
use std::cmp::Ordering;

/// Represents a buy or sell that was recorded by the user. Aggregated into a
/// series of lots. We gather all actions before tabulating them into lots,
/// because we do not require ledger input to be in order.
#[derive(Clone, Debug, Eq)]
pub struct Action {
	pub direction: Direction,
	pub date: Date,

	pub account: String,
	pub commodity: Commodity,
	pub quantity: Quant,

	/// User-defined string, used to force specific lot matching
	pub lot_name: Option<String>,
}

impl Action {
	pub fn new(
		date: Date,
		account: String,
		amount: Amount,
		cost_basis: Amount,
		lot_name: Option<String>,
	) -> Result<Self, Error> {
		if amount.value == 0 {
			bail!("Action cannot have zero quantity")
		}

		let (direction, quantity) = if amount.value > 0 {
			(Direction::Buy, amount.value)
		} else {
			(Direction::Sell(None), -amount.value)
		};

		Ok(Self {
			direction,
			date,
			account,
			quantity,
			commodity: Commodity::new(amount.currency, cost_basis),
			lot_name,
		})
	}

	/// Adds unit proceeds to this if it is a sell. Panics if this is not
	/// a sell or if unit proceeds have already been added to it.
	pub fn add_unit_proceeds(&mut self, unit_proceeds: Amount) {
		if self.direction != Direction::Sell(None) {
			panic!("Buy action cannot have unit proceeds")
		}

		self.direction = Direction::Sell(Some(unit_proceeds));
	}
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Direction {
	Buy,

	/// Contains unit proceeds for the sale, if known, which can then be
	/// used on reports to calculate profit & loss, etc., against a lot.
	Sell(Option<Amount>),
}

impl PartialOrd for Direction {
	fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
		Some(self.cmp(other))
	}
}

impl Ord for Direction {
	fn cmp(&self, other: &Self) -> Ordering {
		match (self, other) {
			(Direction::Buy, Direction::Buy) => Ordering::Equal,
			(Direction::Buy, Direction::Sell(_)) => Ordering::Less, // Buy comes before Sell
			(Direction::Sell(_), Direction::Buy) => Ordering::Greater, // Sell comes after Buy
			(Direction::Sell(a), Direction::Sell(b)) => {
				// Compare unit proceeds if both are Sell
				match (a, b) {
					(Some(a_amount), Some(b_amount)) => a_amount.cmp(b_amount),
					(None, Some(_)) => Ordering::Greater, // Sell with no proceeds sorts higher
					(Some(_), None) => Ordering::Less,
					(None, None) => Ordering::Equal,
				}
			},
		}
	}
}

impl PartialEq for Action {
	fn eq(&self, other: &Self) -> bool {
		self.date == other.date
			&& self.direction == other.direction
			&& self.commodity == other.commodity
	}
}

impl PartialOrd for Action {
	fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
		Some(self.cmp(other))
	}
}

impl Ord for Action {
	fn cmp(&self, other: &Self) -> Ordering {
		// Sort by date (ascending)
		self.date
			.cmp(&other.date)
			// Then by direction: buys first
			.then_with(|| self.direction.cmp(&other.direction))
			// Then by quantity (descending)
			.then_with(|| other.quantity.cmp(&self.quantity))
			// Then by commodity (alphabetical)
			.then_with(|| self.commodity.cmp(&other.commodity))
			// Then by account (alphabetical)
			.then_with(|| self.account.cmp(&other.account))
			// Finally, by lot_name: present sorts lower than absent
			.then_with(|| match (&self.lot_name, &other.lot_name) {
				(Some(a), Some(b)) => a.cmp(b), // Compare alphabetically if both present
				(None, Some(_)) => Ordering::Greater, // Absent sorts higher
				(Some(_), None) => Ordering::Less,
				(None, None) => Ordering::Equal,
			})
	}
}
