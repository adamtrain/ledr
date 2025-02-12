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
use crate::util::quant::Quant;
use std::fmt;
use std::ops::Neg;

/// A quant value with a currency.
#[derive(Clone, Debug, Eq, PartialEq, Hash, Ord, PartialOrd)]
pub struct Amount {
	pub currency: String,
	pub value: Quant,
}

impl Amount {
	pub fn new(value: Quant, currency: &str) -> Self {
		Self {
			value,
			currency: currency.to_string(),
		}
	}

	pub fn zero(currency: &str) -> Self {
		Self {
			value: Quant::zero(),
			currency: currency.to_string(),
		}
	}
}

impl Neg for Amount {
	type Output = Amount;
	fn neg(self) -> Self::Output {
		Self::Output {
			currency: self.currency.to_owned(),
			value: -self.value,
		}
	}
}

impl fmt::Display for Amount {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		write!(f, "{} {}", self.value, self.currency)
	}
}
