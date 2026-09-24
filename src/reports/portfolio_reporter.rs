/* Copyright © 2024-2026 Adam Train <adam@adametrain.com>
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
use crate::investment::sale::Sale;
use crate::reports::table::Table;
use crate::util::amount::DEFAULT_PRECISION;
use crate::util::date::{Date, Duration};
use crate::util::quant::Quant;
use std::collections::BTreeMap;

/// Lots, ready to report on. Values are kept exact throughout; precision
/// only affects how they are shown, so rounding never compounds into totals.
pub struct PortfolioReporter {
	lots: Vec<Lot>,
	max_precision_by_currency: BTreeMap<String, u32>,
	max_precision_allowed: u32,
}

/// One sale out of one lot
#[derive(Debug)]
pub struct RealizedRow {
	pub lot_id: String,
	pub asset: String,
	pub opened: Date,
	pub closed: Date,
	pub held: Duration,
	pub quantity: Quant,
	/// Unit cost basis and its currency
	pub cost: Quant,
	pub cost_currency: String,
	/// Unit proceeds as recorded, if known, with their currency
	pub proceeds: Option<(Quant, String)>,
	/// Gain in the cost basis currency, if the proceeds could be valued in it
	pub gain: Option<Gain>,
}

/// An open lot
#[derive(Debug)]
pub struct UnrealizedRow {
	pub lot_id: String,
	pub asset: String,
	pub opened: Date,
	pub held: Duration,
	pub quantity: Quant,
	pub cost: Quant,
	pub cost_currency: String,
	pub gain: Option<UnrealizedGain>,
}

/// Rows plus totals per cost basis currency. A total is None when any row
/// in its currency has an unknown gain, since the sum would be misleading.
#[derive(Debug)]
pub struct GainReport<Row> {
	pub rows: Vec<Row>,
	pub totals: BTreeMap<String, Option<Quant>>,
	/// Total cost basis per currency, of the quantities reported
	pub costs: BTreeMap<String, Quant>,
}

impl PortfolioReporter {
	pub fn new(
		mut lots: Vec<Lot>,
		max_precision_by_currency: BTreeMap<String, u32>,
		max_precision_allowed: u32,
	) -> Self {
		lots.sort();
		Self {
			lots,
			max_precision_by_currency,
			max_precision_allowed,
		}
	}

	/// Decimal places to show for a currency
	pub fn precision(&self, currency: &str) -> u32 {
		let allowed = self.max_precision_allowed;
		self.max_precision_by_currency
			.get(currency)
			.copied()
			.unwrap_or(if allowed == u32::MAX {
				DEFAULT_PRECISION
			} else {
				allowed
			})
			.min(allowed)
	}

	/// Rounds a value for display in the given currency
	pub fn show(&self, value: Quant, currency: &str) -> String {
		let mut value = value;
		value.round(self.precision(currency));
		value.to_string()
	}

	fn show_amount(&self, value: Quant, currency: &str) -> String {
		format!("{} {currency}", self.show(value, currency))
	}

	/// Every sale between `begin` and `end`, inclusive
	pub fn realized(
		&self,
		begin: &Date,
		end: &Date,
		exchange_rates: &ExchangeRates,
	) -> GainReport<RealizedRow> {
		let mut report = GainReport {
			rows: vec![],
			totals: BTreeMap::new(),
			costs: BTreeMap::new(),
		};

		for lot in &self.lots {
			for sale in &lot.sales {
				if &sale.date < begin || &sale.date > end {
					continue;
				}
				let cb = lot.commodity.cost_basis();
				let gain = realized_gain(lot, sale, exchange_rates);
				report.add(
					&cb.currency,
					cb.value * sale.quantity,
					gain.as_ref().map(|g| g.total),
				);
				report.rows.push(RealizedRow {
					lot_id: lot.id.clone(),
					asset: lot.commodity.symbol().to_string(),
					opened: lot.acquisition_date,
					closed: sale.date,
					held: sale.time_held(&lot.acquisition_date),
					quantity: sale.quantity,
					cost: cb.value,
					cost_currency: cb.currency.clone(),
					proceeds: sale
						.unit_proceeds
						.as_ref()
						.map(|p| (p.value, p.currency.clone())),
					gain,
				});
			}
		}
		report
	}

	/// Every open lot, valued at the latest known prices
	pub fn unrealized(
		&self,
		as_of: &Date,
		exchange_rates: &ExchangeRates,
	) -> GainReport<UnrealizedRow> {
		let mut report = GainReport {
			rows: vec![],
			totals: BTreeMap::new(),
			costs: BTreeMap::new(),
		};

		for lot in &self.lots {
			let cb = lot.commodity.cost_basis();
			let gain = unrealized_gain(lot, exchange_rates);
			report.add(
				&cb.currency,
				cb.value * lot.quantity,
				gain.as_ref().map(|g| g.total),
			);
			report.rows.push(UnrealizedRow {
				lot_id: lot.id.clone(),
				asset: lot.commodity.symbol().to_string(),
				opened: lot.acquisition_date,
				held: lot.time_held(as_of),
				quantity: lot.quantity,
				cost: cb.value,
				cost_currency: cb.currency.clone(),
				gain,
			});
		}
		report
	}

	/// A plain table of realized gains and losses
	pub fn plain_realized(&self, report: &GainReport<RealizedRow>) -> String {
		if self.lots.is_empty() {
			return "No applicable lots\n".to_string();
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

		for row in &report.rows {
			let (unit, total, notes) = match &row.gain {
				Some(gain) => (
					self.show_amount(gain.unit, &row.cost_currency),
					self.show_amount(gain.total, &row.cost_currency),
					match (&gain.rate, &row.proceeds) {
						(Some(rate), Some((_, proceeds_currency))) => format!(
							"Unit G/L est. from rate of {} {}/{}",
							show_rate(*rate),
							row.cost_currency,
							proceeds_currency
						),
						_ => String::new(),
					},
				),
				None => ("UNK".into(), "UNK".into(), String::new()),
			};
			let proceeds = match &row.proceeds {
				Some((value, currency)) => self.show_amount(*value, currency),
				None => "UNK".into(),
			};

			table.add_row(vec![
				&row.lot_id,
				&row.opened.to_string(),
				&row.closed.to_string(),
				&row.held.to_string(),
				&row.asset,
				&self.show(row.quantity, &row.asset),
				&self.show_amount(row.cost, &row.cost_currency),
				&proceeds,
				&unit,
				&total,
				&notes,
			])
		}

		table.add_partial_separator(vec![9]);
		for shown in self.plain_totals(report) {
			table.add_row(vec!["", "", "", "", "", "", "", "", "", &shown, ""]);
		}
		table.render()
	}

	/// A plain table of unrealized gains and losses
	pub fn plain_unrealized(
		&self,
		report: &GainReport<UnrealizedRow>,
	) -> String {
		if self.lots.is_empty() {
			return "No applicable lots\n".to_string();
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

		for row in &report.rows {
			let (latest, unit, total) = match &row.gain {
				Some(g) => (
					self.show_amount(g.price, &row.cost_currency),
					self.show_amount(g.unit, &row.cost_currency),
					self.show_amount(g.total, &row.cost_currency),
				),
				None => ("UNK".into(), "UNK".into(), "UNK".into()),
			};
			table.add_row(vec![
				&row.lot_id,
				&row.opened.to_string(),
				&row.held.to_string(),
				&row.asset,
				&self.show(row.quantity, &row.asset),
				&self.show_amount(row.cost, &row.cost_currency),
				&latest,
				&unit,
				&total,
			])
		}

		table.add_partial_separator(vec![8]);
		for shown in self.plain_totals(report) {
			table.add_row(vec!["", "", "", "", "", "", "", "", &shown]);
		}
		table.render()
	}

	fn plain_totals<Row>(&self, report: &GainReport<Row>) -> Vec<String> {
		report
			.totals
			.iter()
			.map(|(currency, total)| match total {
				Some(t) => self.show_amount(*t, currency),
				None => "UNK".to_string(),
			})
			.collect()
	}
}

impl<Row> GainReport<Row> {
	fn add(&mut self, currency: &str, cost: Quant, gain: Option<Quant>) {
		*self.costs.entry(currency.to_string()).or_default() += cost;
		let total = self
			.totals
			.entry(currency.to_string())
			.or_insert(Some(Quant::zero()));
		match (total.as_mut(), gain) {
			(Some(t), Some(g)) => *t += g,
			_ => *total = None,
		}
	}
}

/// An exchange rate, shown with enough places to be meaningful
pub fn show_rate(rate: Quant) -> String {
	let mut rate = rate;
	rate.set_render_precision(2, true);
	rate.make_visible();
	rate.to_string()
}

/// A gain or loss, per unit and in total, in the lot's cost basis currency.
#[derive(Debug)]
pub struct Gain {
	pub unit: Quant,
	pub total: Quant,
	/// The exchange rate used to convert proceeds into the cost basis
	/// currency, if they were in another currency
	pub rate: Option<Quant>,
}

/// The realized gain on one sale from a lot, if its proceeds are known and
/// can be valued in the lot's cost basis currency as of the sale date.
pub fn realized_gain(
	lot: &Lot,
	sale: &Sale,
	exchange_rates: &ExchangeRates,
) -> Option<Gain> {
	let cb = lot.commodity.cost_basis();
	let proceeds = sale.unit_proceeds.as_ref()?;

	let (unit_proceeds, rate) = if proceeds.currency == cb.currency {
		(proceeds.value, None)
	} else {
		let rate = exchange_rates.get_rate_as_of(
			&proceeds.currency,
			&cb.currency,
			&sale.date,
		)?;
		(proceeds.value * rate, Some(rate))
	};

	let unit = unit_proceeds - cb.value;
	Some(Gain {
		unit,
		total: unit * sale.quantity,
		rate,
	})
}

/// An open lot's unrealized gain at the latest known price.
#[derive(Debug)]
pub struct UnrealizedGain {
	pub price: Quant,
	pub unit: Quant,
	pub total: Quant,
}

/// The unrealized gain on what remains of a lot, if there is a price for it.
pub fn unrealized_gain(
	lot: &Lot,
	exchange_rates: &ExchangeRates,
) -> Option<UnrealizedGain> {
	let cb = lot.commodity.cost_basis();
	let price =
		exchange_rates.get_latest_rate(lot.commodity.symbol(), &cb.currency)?;
	let unit = price - cb.value;
	Some(UnrealizedGain {
		price,
		unit,
		total: unit * lot.quantity,
	})
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::investment::commodity::Commodity;
	use crate::investment::lot::LotStatus;
	use crate::util::amount::Amount;

	fn lot(cost: i128, quantity: i128) -> Lot {
		Lot {
			id: "1".into(),
			is_named: false,
			status: LotStatus::Open,
			account: "Assets:Broker".into(),
			commodity: Commodity::new(
				"AAPL".into(),
				Amount::new(Quant::from_i128(cost), "USD"),
			),
			quantity: Quant::from_i128(quantity),
			acquisition_date: Date::from_str("2024-01-01").unwrap(),
			closed_date: None,
			sales: vec![],
		}
	}

	fn rates() -> ExchangeRates {
		let mut rates = ExchangeRates::new(false);
		rates.finalize(&BTreeMap::new()).unwrap();
		rates
	}

	#[test]
	fn test_realized_gain_is_proceeds_minus_cost() {
		// Bought at 100, sold at 150: a gain of 50 each, 500 in all
		let sale = Sale {
			date: Date::from_str("2024-06-01").unwrap(),
			quantity: Quant::from_i128(10),
			unit_proceeds: Some(Amount::new(Quant::from_i128(150), "USD")),
		};
		let gain = realized_gain(&lot(100, 10), &sale, &rates()).unwrap();
		assert_eq!(gain.unit, Quant::from_i128(50));
		assert_eq!(gain.total, Quant::from_i128(500));
	}

	#[test]
	fn test_realized_loss() {
		let sale = Sale {
			date: Date::from_str("2024-06-01").unwrap(),
			quantity: Quant::from_i128(2),
			unit_proceeds: Some(Amount::new(Quant::from_i128(80), "USD")),
		};
		let gain = realized_gain(&lot(100, 10), &sale, &rates()).unwrap();
		assert_eq!(gain.total, Quant::from_i128(-40));
	}

	#[test]
	fn test_unknown_proceeds_have_no_gain() {
		let sale = Sale {
			date: Date::from_str("2024-06-01").unwrap(),
			quantity: Quant::from_i128(2),
			unit_proceeds: None,
		};
		assert!(realized_gain(&lot(100, 10), &sale, &rates()).is_none());
	}

	#[test]
	fn test_unpriced_lot_has_no_unrealized_gain() {
		assert!(unrealized_gain(&lot(100, 10), &rates()).is_none());
	}

	#[test]
	fn test_totals_are_unknown_if_any_gain_is() {
		let mut report: GainReport<()> = GainReport {
			rows: vec![],
			totals: BTreeMap::new(),
			costs: BTreeMap::new(),
		};
		report.add("USD", Quant::from_i128(1), Some(Quant::from_i128(5)));
		report.add("USD", Quant::from_i128(1), None);
		report.add("CAD", Quant::from_i128(1), Some(Quant::from_i128(5)));
		assert_eq!(report.totals["USD"], None);
		assert_eq!(report.totals["CAD"], Some(Quant::from_i128(5)));
	}
}
