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

use crate::diagnostics::Diagnostic;
use crate::gl::observed_rate::{ObservationType, ObservedRate};
use crate::util::amount::Amount;
use crate::util::date::Date;
use crate::util::graph::Graph;
use crate::util::quant::Quant;
use anyhow::{Error, bail};
use std::cell::RefCell;
use std::collections::{BTreeMap, HashSet};

/// How far an observed rate may stray from the rate declared for the same
/// date, or a cycle of rates from multiplying out to 1, before it is flagged.
pub const RATE_TOLERANCE_PERCENT: i128 = 5;

/// An observed conversion that strays too far from a declared rate.
#[derive(Clone, Debug, PartialEq)]
pub struct RateConflict {
	pub base: String,
	pub quote: String,
	/// Declared units of quote per unit of base
	pub declared: Quant,
	/// Observed units of quote per unit of base
	pub observed: Quant,
}

impl RateConflict {
	fn show(rate: Quant) -> String {
		let mut rate = rate;
		rate.set_render_precision(0, true);
		format!("{rate:.4}")
	}

	pub fn declared_display(&self) -> String {
		Self::show(self.declared)
	}

	pub fn observed_display(&self) -> String {
		Self::show(self.observed)
	}

	pub fn deviation_display(&self) -> String {
		let mut percent =
			((self.observed / self.declared) - Quant::from_i128(1)).abs() * 100;
		percent.round(0);
		format!("{percent}%")
	}
}

#[derive(Debug)]
pub struct ExchangeRates {
	/// Stores a set of Graphs, one per date
	daily_graphs: BTreeMap<Date, Graph>,

	/// Graph that incorporates the most recent date of observations for each
	/// pair; less accurate across many hops, but much more complete.
	primary_graph: Graph,

	is_finalized: bool,

	/// Whether to run expensive consistency checks while finalizing
	thorough: bool,

	/// Preprocessed data for performant lookups, only available after
	/// finalize() has been called on this.
	resolved_rates: BTreeMap<(String, String), Vec<ObservedRate>>,

	/// Set of currencies the user has told us has no value
	worthless: HashSet<String>,

	/// Rates looked up in the primary graph so far, since each lookup
	/// searches the graph
	across_dates: RefCell<BTreeMap<(String, String), Option<Quant>>>,
}

impl ExchangeRates {
	pub fn new(thorough: bool) -> Self {
		Self {
			daily_graphs: Default::default(),
			resolved_rates: Default::default(),
			primary_graph: Graph::new_undated(),
			is_finalized: false,
			worthless: Default::default(),
			across_dates: Default::default(),
			thorough,
		}
	}

	/// Adds a rate in base-quote semantics. Reports a conflict if this is
	/// an observation far from a rate declared for the same date.
	pub fn add_rate(
		&mut self,
		date: Date,
		base: String,
		quote: String,
		rate: Quant,
		observation_type: ObservationType,
	) -> Result<Option<RateConflict>, Error> {
		if self.worthless.contains(&base) || self.worthless.contains(&quote) {
			return Ok(None);
		}

		let b_amt = Amount::new(Quant::from_i128(1), &base);
		let q_amt = Amount::new(rate, &quote);

		self.add_equality(date, b_amt, q_amt, observation_type)
	}

	/// Adds a rate by passing two amounts of different currencies that are
	/// deemed to be identical in value to each other. Reports a conflict if
	/// this is an observation far from a rate declared for the same date.
	pub fn add_equality(
		&mut self,
		date: Date,
		a: Amount,
		b: Amount,
		observation_type: ObservationType,
	) -> Result<Option<RateConflict>, Error> {
		if a.currency == b.currency {
			bail!("Cannot exchange a currency for itself")
		}

		// We conceptually can't add zero-value rates; infinities are bad,
		// and we also no-op if the user has declared something worthless;
		// why convert it to anything in that case?
		if a.value == 0
			|| b.value == 0
			|| self.worthless.contains(&b.currency)
			|| self.worthless.contains(&a.currency)
		{
			return Ok(None);
		}

		let graph = self
			.daily_graphs
			.entry(date)
			.or_insert_with(|| Graph::new(date));

		if let Some(declared) =
			graph.get_direct_rate(&a.currency, &b.currency, true)
		{
			match observation_type {
				ObservationType::Declared => {
					bail!(
						"Cannot declare multiple rates between {} and {} on {date}",
						a.currency,
						b.currency
					)
				},
				ObservationType::Inferred => {
					// The declaration stands, but an observation far from it
					// is probably a mistake in one or the other
					let observed = b.value / a.value;
					let deviation = ((observed / declared)
						- Quant::from_i128(1))
					.abs() * 100;
					let conflict =
						(deviation > RATE_TOLERANCE_PERCENT).then(|| {
							RateConflict {
								base: a.currency.clone(),
								quote: b.currency.clone(),
								declared,
								observed,
							}
						});
					return Ok(conflict);
				},
				ObservationType::Direct => unreachable!(),
			}
		}

		graph.add_rate(&date, &a, &b, observation_type)?;

		self.primary_graph.overwrite_rate_if_newer(
			&date,
			&a,
			&b,
			observation_type,
		)?;

		Ok(None)
	}

	/// Reports that the currency in question has no value and should always
	/// be reported as having zero worth.
	///
	/// If the currency is already in the graphs, it and all edges to & from it
	/// are deleted.
	pub fn declare_worthless(&mut self, symbol: String) {
		for graph in self.daily_graphs.values_mut() {
			graph.remove_currency(&symbol);
		}
		self.primary_graph.remove_currency(&symbol);

		self.worthless.insert(symbol);
	}

	/// Finalizes the rates into a resolved form for efficient lookups,
	/// after which the methods to retrieve rates from here will work.
	/// Prior to that, they will not work. Finalization can fail if any
	/// of the underlying Graphs are incoherent.
	pub fn finalize(
		&mut self,
		max_precision_by_currency: &BTreeMap<String, u32>,
	) -> Result<Vec<Diagnostic>, Error> {
		let mut resolved = BTreeMap::new();
		let mut warnings = vec![];

		for (date, graph) in &self.daily_graphs {
			if self.thorough && graph.has_inconsistent_cycle() {
				warnings.push(
					Diagnostic::warning(format!(
						"Exchange rates on {date} are not consistent with each other"
					))
					.note(format!(
						"converting around a loop of currencies on this date \
						changes the value by more than {RATE_TOLERANCE_PERCENT}%"
					))
					.help(
						"check the rate directives and conversions on this date",
					),
				);
			}

			// Make sure exchange rates inherit desired precision from user
			for (base, quote, mut observation) in graph.get_all_rates() {
				if let Some(precision) = determine_precision(
					max_precision_by_currency.get(&base),
					max_precision_by_currency.get(&quote),
				) {
					observation.rate.set_render_precision(precision, true);
				}

				resolved
					.entry((base.clone(), quote.clone()))
					.or_insert_with(Vec::new)
					.push(observation);
			}
		}

		// Sort rates for each pair by date (descending) and then by rate
		for rates in resolved.values_mut() {
			rates.sort_by(|a, b| {
				b.date.cmp(&a.date).then_with(|| a.rate.cmp(&b.rate))
			});
		}

		self.resolved_rates = resolved;
		self.is_finalized = true;

		Ok(warnings)
	}

	/// Retrieves the most recent rate, if any, at or before the given date.
	/// Only rates known on a date count; the undated rates pieced together
	/// across dates could come from after it.
	pub fn get_rate_as_of(
		&self,
		base: &str,
		quote: &str,
		as_of: &Date,
	) -> Option<Quant> {
		if !self.is_finalized {
			panic!("exchange rates not finalized")
		};

		self.resolved_rates
			.get(&(base.to_string(), quote.to_string()))
			.and_then(|rates| {
				rates
					.iter()
					.find(|o| o.date.is_some_and(|d| d <= *as_of))
					.map(|o| o.rate)
			})
	}

	/// Retrieves the most recent rate available, if any. Pairs never
	/// connected on the same date fall back to the graph of the most recent
	/// rate between each pair of currencies, which is less accurate across
	/// many hops, but much more complete.
	pub fn get_latest_rate(&self, base: &str, quote: &str) -> Option<Quant> {
		if !self.is_finalized {
			panic!("exchange rates not finalized")
		};

		let key = (base.to_string(), quote.to_string());
		if let Some(rates) = self.resolved_rates.get(&key) {
			return rates.first().map(|o| o.rate);
		}
		*self
			.across_dates
			.borrow_mut()
			.entry(key)
			.or_insert_with(|| self.primary_graph.convert_one(base, quote))
	}

	/// Returns the final map of resolved rates. Consumes this.
	pub fn take_all_rates(
		self,
	) -> BTreeMap<(String, String), Vec<ObservedRate>> {
		if !self.is_finalized {
			panic!("exchange rates not finalized")
		};

		self.resolved_rates
	}
}

/// Helper function to determine the maximum precision between two optionals
fn determine_precision(
	base_precision: Option<&u32>,
	quote_precision: Option<&u32>,
) -> Option<u32> {
	match (base_precision, quote_precision) {
		(Some(&bp), Some(&qp)) => Some(bp.max(qp)),
		(Some(&bp), None) => Some(bp),
		(None, Some(&qp)) => Some(qp),
		(None, None) => None,
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::util::date::Date;
	use crate::util::quant::Quant;

	#[test]
	fn test_declare_valid_rate() {
		let mut exchange_rates = ExchangeRates::new(false);
		let date = Date::from_str("2024-1-1").unwrap();
		let base = "USD".to_string();
		let quote = "EUR".to_string();
		let rate = Quant::new(11, 1);

		assert!(
			exchange_rates
				.add_rate(
					date,
					base.clone(),
					quote.clone(),
					rate,
					ObservationType::Declared
				)
				.is_ok()
		);

		let date2 = Date::from_str("2024-11-2").unwrap();
		assert!(
			exchange_rates
				.add_rate(
					date2,
					base,
					quote,
					Quant::new(12, 1),
					ObservationType::Declared
				)
				.is_ok()
		);
	}

	#[test]
	fn test_declare_self_exchange() {
		let mut exchange_rates = ExchangeRates::new(false);
		let date = Date::from_str("2024-1-1").unwrap();
		let base = "USD".to_string();
		let rate = Quant::new(11, 1);

		assert!(
			exchange_rates
				.add_rate(
					date,
					base.clone(),
					base.clone(),
					rate,
					ObservationType::Declared
				)
				.is_err()
		);

		let date2 = Date::from_str("2024-11-2").unwrap();
		assert!(
			exchange_rates
				.add_rate(
					date2,
					base.clone(),
					base,
					Quant::new(9, 1),
					ObservationType::Declared
				)
				.is_err()
		);
	}

	#[test]
	fn test_declare_non_positive_rate() {
		let mut exchange_rates = ExchangeRates::new(false);
		let date = Date::from_str("2024-11-01").unwrap();
		let base = "USD".to_string();
		let quote = "EUR".to_string();

		assert!(
			exchange_rates
				.add_rate(
					date,
					base.clone(),
					quote.clone(),
					Quant::new(0, 0),
					ObservationType::Declared
				)
				.is_ok()
		);
		assert!(
			exchange_rates
				.add_rate(
					date,
					base,
					quote,
					Quant::new(-1, 1),
					ObservationType::Declared
				)
				.is_ok()
		);
	}

	#[test]
	fn test_infer_rate_within_tolerance() {
		let mut exchange_rates = ExchangeRates::new(false);
		let date = Date::from_str("2024-11-01").unwrap();
		let base = "USD".to_string();
		let quote = "EUR".to_string();
		let declared_rate = Quant::new(11, 1);

		exchange_rates
			.add_rate(
				date,
				base.clone(),
				quote.clone(),
				declared_rate,
				ObservationType::Declared,
			)
			.unwrap();

		let inferred_rate = Quant::new(1099, 3);
		assert!(
			exchange_rates
				.add_rate(
					date,
					base.clone(),
					quote.clone(),
					inferred_rate,
					ObservationType::Inferred
				)
				.is_ok()
		);

		let date2 = Date::from_str("2024-11-02").unwrap();
		assert!(
			exchange_rates
				.add_rate(
					date2,
					base.clone(),
					quote.clone(),
					Quant::new(111, 2),
					ObservationType::Inferred
				)
				.is_ok()
		);
	}
}
