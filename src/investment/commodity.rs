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
use crate::util::amount::Amount;
use std::fmt::Formatter;

#[derive(Clone, Debug, Eq, Hash, PartialEq, PartialOrd, Ord)]
pub struct Commodity {
	symbol: String,

	/// Always in unit terms
	cost_basis: Amount,
}

impl Commodity {
	pub fn new(symbol: String, cost_basis: Amount) -> Self {
		Self { symbol, cost_basis }
	}

	pub fn symbol(&self) -> &str {
		&self.symbol
	}

	pub fn cost_basis(&self) -> &Amount {
		&self.cost_basis
	}
}

impl std::fmt::Display for Commodity {
	fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
		write!(f, "{} {{ {} }}", self.symbol, self.cost_basis)
	}
}
