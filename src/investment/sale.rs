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
use crate::util::amount::Amount;
use crate::util::date::{Date, Duration};
use crate::util::quant::Quant;

#[derive(Debug, PartialEq, Eq, PartialOrd)]
pub struct Sale {
	pub date: Date,
	pub quantity: Quant,

	/// Always conceptually exists, but only present if known;
	/// not necessarily the currency of acquisition
	pub unit_proceeds: Option<Amount>,
}

impl Sale {
	pub fn time_held(&self, acquisition_date: &Date) -> Duration {
		acquisition_date.until(&self.date)
	}
}
