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
use crate::investment::action::Action;
use crate::investment::lot::{Lot, LotStatus};
use crate::investment::sale::Sale;
use crate::util::amount::Amount;
use anyhow::{bail, Error};

/// A set of lots, reflecting a certain coherent state of an investment
/// portfolio. When it is being assembled, it will reject states where,
/// for example, more is sold than one owns. (Negative cost bases and
/// liability accounts should be sufficient for edge cases in these
/// areas).
///
/// Once assembled, it can filter and ultimately provide its set of
/// lots to another struct for reporting.
pub struct Portfolio {
	state: Vec<Lot>,
	/// The ID number that will be automatically assigned to the next lot
	next_id: u64,
}

impl Portfolio {
	pub fn new() -> Self {
		Self {
			state: Default::default(),
			next_id: 0,
		}
	}

	pub fn buy_lot(&mut self, action: &Action) {
		let id = match action.lot_name {
			Some(ref name) => name.clone(),
			None => {
				self.next_id += 1;
				self.next_id.to_string()
			},
		};

		self.state.push(Lot {
			id,
			is_named: action.lot_name.is_some(),
			status: LotStatus::Open,
			account: action.account.clone(),
			commodity: action.commodity.clone(),
			quantity: action.quantity,
			acquisition_date: action.date,
			closed_date: None,
			sales: vec![],
		});
	}

	pub fn sell_lot(
		&mut self,
		action: &Action,
		unit_proceeds: Option<Amount>,
	) -> Result<(), Error> {
		let mut remaining_quantity = action.quantity;

		for lot in self.state.iter_mut() {
			if lot.commodity != action.commodity
				|| lot.status == LotStatus::Closed
			{
				continue;
			}

			// If a lot is named, we only sell against the matching lot
			if let Some(name) = &action.lot_name {
				if name != &lot.id {
					continue;
				}
			}

			// Determine how much can be sold from this lot
			let sell_quantity = remaining_quantity.min(lot.quantity);
			lot.quantity -= sell_quantity;
			remaining_quantity -= sell_quantity;

			// Register the sale against the lot
			lot.sales.push(Sale {
				date: action.date,
				quantity: sell_quantity,
				unit_proceeds: unit_proceeds.clone(),
			});

			// If the lot is fully sold, mark it as closed
			if lot.quantity == 0 {
				lot.status = LotStatus::Closed;
				lot.closed_date = Some(action.date);
			}

			// Break if we've sold everything needed
			if remaining_quantity == 0 {
				break;
			}
		}

		if remaining_quantity > 0 {
			bail!(
				"No remaining lots for {} of {} (cost basis {})",
				remaining_quantity,
				action.commodity.symbol(),
				action.commodity.cost_basis(),
			)
		}

		Ok(())
	}

	/// Flattens the set of lots into one Vec, applies filters, and returns it.
	/// Consumes this.
	pub fn take_lots(
		self,
		filters: impl IntoIterator<Item = LotFilter>,
	) -> Vec<Lot> {
		// First assign IDs to all lots

		let mut lots_iter: Box<dyn Iterator<Item = Lot>> =
			Box::new(self.state.into_iter());

		for filter in filters {
			lots_iter = match filter {
				LotFilter::Status(status) => {
					Box::new(lots_iter.filter(move |lot| lot.status == status))
				},
				LotFilter::HasSales(has_sales) => Box::new(
					lots_iter
						.filter(move |lot| lot.sales.is_empty() != has_sales),
				),
			};
		}

		lots_iter.collect()
	}
}

/// Pass these into the take_lots() method to filter the output.
pub enum LotFilter {
	Status(LotStatus),
	HasSales(bool),
}
