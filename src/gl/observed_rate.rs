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
use crate::util::quant::Quant;

/// An exchange rate between two currencies, observed on a specific date.
/// A value object not intended to have much functionality.
#[derive(Debug)]
pub struct ObservedRate {
	pub rate: Quant,
	pub observation_type: ObservationType,

	/// Some rates are inferred over a range and have nonspecific date
	pub date: Option<Date>,
}

impl ObservedRate {
	pub fn new(
		rate: Quant,
		date: Option<Date>,
		observation_type: ObservationType,
	) -> Self {
		Self {
			rate,
			date,
			observation_type,
		}
	}

	/// Ensures the exchange rate is rendered as nonzero when printed
	/// by increasing its precision.
	pub fn make_visible(&mut self) {
		self.rate.make_visible();
	}
}

/// The nature of an observation of an exchange rate.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ObservationType {
	/// The user told us this rate in the abstract, so it is gospel
	Declared,
	/// The user reports these currencies traded for this amount
	Direct,
	/// The graph says that this rate is sensible from traversal
	Inferred,
}
