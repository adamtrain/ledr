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
use crate::gl::exchange_rates::ExchangeRates;
use crate::investment::lot::Lot;
use crate::reports::table::Table;
use crate::util::amount::Amount;
use crate::util::date::Date;
use crate::util::quant::Quant;
use std::collections::BTreeMap;

/// Struct for handling and displaying an ordered list of lots, for reports
pub struct PortfolioReporter {
	lots: Vec<Lot>,
}

impl PortfolioReporter {
	/// When this inits, it will sanitize its inputs with the passed
	/// parameters, and then store rounded lots for pretty reporting.
	pub fn new(
		mut lots: Vec<Lot>,
		max_precision_by_currency: BTreeMap<String, u32>,
		max_precision_allowed: u32,
	) -> Self {
		lots.sort();

		for lot in &mut lots {
			round_to_precision(
				&mut lot.quantity,
				lot.commodity.symbol(),
				&max_precision_by_currency,
				max_precision_allowed,
			);

			for sale in &mut lot.sales {
				round_to_precision(
					&mut sale.quantity,
					lot.commodity.symbol(),
					&max_precision_by_currency,
					max_precision_allowed,
				);

				if let Some(ref mut proceeds) = sale.unit_proceeds {
					round_to_precision(
						&mut proceeds.value,
						&proceeds.currency,
						&max_precision_by_currency,
						max_precision_allowed,
					);
				}
			}
		}

		Self { lots }
	}

	/// Prints a realized gain/loss report.
	pub fn print_realized_gain_loss(
		&self,
		begin: &Date,
		end: &Date,
		exchange_rates: &ExchangeRates,
	) {
		if self.lots.is_empty() {
			println!("No applicable lots");
			return;
		}

		let mut table = Table::new(11);
		table.right_align(vec![0, 1, 2, 3, 4, 5, 6, 7, 8, 9]);

		table.add_header(vec![
			"ID",
			"Opened",
			"Closed",
			"Held",
			"Asset",
			"Qty",
			"Cost",
			"Proceeds",
			"Unit G/L",
			"Total G/L",
			"Notes",
		]);

		table.add_separator();

		// currency -> total g/l
		let mut totals: BTreeMap<String, Amount> = BTreeMap::new();
		let mut has_any_unknown_gl = false;

		for l in &self.lots {
			for s in &l.sales {
				if &s.date < begin || &s.date > end {
					continue;
				}

				let cb = l.commodity.cost_basis();
				let mut proceeds_str = "UNK".to_string();
				let mut notes = "".to_string();

				let pr = if let Some(pr) = &s.unit_proceeds {
					proceeds_str = pr.to_string();

					if cb.currency == pr.currency {
						Some(Amount::new(pr.value - cb.value, &cb.currency))
					} else {
						match exchange_rates.get_rate_as_of(
							&pr.currency,
							&cb.currency,
							end,
						) {
							Some(amt) => {
								notes = format!(
									"Unit G/L est. from rate of {} {}/{}",
									amt, cb.currency, pr.currency
								);
								Some(Amount::new(amt * pr.value, &cb.currency))
							},
							None => {
								has_any_unknown_gl = true;
								Some(Amount::new(pr.value, &pr.currency))
							},
						}
					}
				} else {
					None
				};

				let (mut unit_gl_str, mut total_gl_str) =
					("UNK".to_string(), "UNK".to_string());

				if let Some(pr) = pr {
					if pr.currency == cb.currency {
						let unit_gl =
							Amount::new(pr.value - cb.value, &cb.currency);
						unit_gl_str = unit_gl.to_string();

						let total_gl = Amount::new(
							unit_gl.value * s.quantity,
							&cb.currency,
						);
						total_gl_str = total_gl.to_string();

						totals
							.entry(cb.currency.clone())
							.or_insert(Amount::zero(&cb.currency))
							.value += total_gl.value;
					} else {
						has_any_unknown_gl = true;
					}
				} else {
					has_any_unknown_gl = true;
				}

				table.add_row(vec![
					&l.id,
					&l.acquisition_date.to_string(),
					&s.date.to_string(),
					&s.time_held(&l.acquisition_date).to_string(),
					l.commodity.symbol(),
					&s.quantity.to_string(),
					&l.commodity.cost_basis().to_string(),
					&proceeds_str,
					&unit_gl_str,
					&total_gl_str,
					&notes,
				])
			}
		}

		table.add_partial_separator(vec![9]);

		// Summing up full total is unknown if any proceeds can't be determined
		if has_any_unknown_gl {
			table.add_row(vec!["", "", "", "", "", "", "", "", "", "UNK", ""]);
		} else {
			let mut sorted_totals: Vec<_> = totals.values().collect();
			sorted_totals.sort_by(|a, b| a.currency.cmp(&b.currency));

			// One line of totals per currency
			for total_gl in sorted_totals {
				table.add_row(vec![
					"",
					"",
					"",
					"",
					"",
					"",
					"",
					"",
					"",
					&total_gl.to_string(),
					"",
				]);
			}
		}

		table.print()
	}

	/// Prints an unrealized gain/loss report.
	pub fn print_unrealized_gain_loss(
		&self,
		as_of: &Date,
		exchange_rates: &ExchangeRates,
	) {
		if self.lots.is_empty() {
			println!("No applicable lots");
			return;
		}

		let mut table = Table::new(9);
		table.right_align(vec![0, 1, 2, 3, 4, 5, 6, 7, 8]);

		table.add_header(vec![
			"ID",
			"Opened",
			"Held",
			"Asset",
			"Qty",
			"Cost",
			"Latest",
			"Unit UG/L",
			"Total UG/L",
		]);

		table.add_separator();

		// currency -> total g/l
		let mut totals: BTreeMap<String, Amount> = BTreeMap::new();

		for l in &self.lots {
			let cb = l.commodity.cost_basis();
			totals
				.entry(cb.clone().currency)
				.or_insert(Amount::zero(&cb.currency))
				.value += cb.value;

			let current = match exchange_rates
				.get_latest_rate(l.commodity.symbol(), &cb.currency)
			{
				Some(r) => Amount::new(r, &cb.currency),
				None => cb.clone(),
			};

			let unit_gl = Amount::new(current.value - cb.value, &cb.currency);

			let total_gl =
				Amount::new(unit_gl.value * l.quantity, &cb.currency);

			table.add_row(vec![
				&l.id,
				&l.acquisition_date.to_string(),
				&l.time_held(as_of).to_string(),
				l.commodity.symbol(),
				&l.quantity.to_string(),
				&l.commodity.cost_basis().to_string(),
				&current.to_string(),
				&unit_gl.to_string(),
				&total_gl.to_string(),
			])
		}

		table.add_partial_separator(vec![8]);

		let mut sorted_totals: Vec<_> = totals.values().collect();
		sorted_totals.sort_by(|a, b| a.currency.cmp(&b.currency));

		// One line of totals per currency
		for total_gl in sorted_totals {
			table.add_row(vec![
				"",
				"",
				"",
				"",
				"",
				"",
				"",
				"",
				&total_gl.to_string(),
			]);
		}

		table.print()
	}
}

fn round_to_precision(
	value: &mut Quant,
	symbol: &str,
	max_precision_by_currency: &BTreeMap<String, u32>,
	max_precision: u32,
) {
	if let Some(precision) = max_precision_by_currency.get(symbol) {
		value.round(*precision.min(&max_precision));
	} else {
		panic!("Missing symbol for lot precision");
	}
}
