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

use crate::gl::observed_rate::{ObservationType, ObservedRate};
use crate::util::amount::Amount;
use crate::util::date::Date;
use crate::util::graph::Graph;
use crate::util::quant::Quant;
use anyhow::{bail, Error};
use std::collections::{BTreeMap, HashSet};

#[derive(Debug)]
pub struct ExchangeRates {
	/// Stores a set of Graphs, one per date
	daily_graphs: BTreeMap<Date, Graph>,

	/// Graph that incorporates the most recent date of observations for each
	/// pair; less accurate across many hops, but much more complete.
	primary_graph: Graph,

	is_finalized: bool,
	allow_warnings: bool,

	/// Preprocessed data for performant lookups, only available after
	/// finalize() has been called on this.
	resolved_rates: BTreeMap<(String, String), Vec<ObservedRate>>,

	/// Set of currencies the user has told us has no value
	worthless: HashSet<String>,
}

impl ExchangeRates {
	pub fn new(warnings: bool) -> Self {
		Self {
			daily_graphs: Default::default(),
			resolved_rates: Default::default(),
			primary_graph: Graph::new_undated(),
			is_finalized: false,
			worthless: Default::default(),
			allow_warnings: warnings,
		}
	}

	/// Adds a rate in base-quote semantics.
	pub fn add_rate(
		&mut self,
		date: Date,
		base: String,
		quote: String,
		rate: Quant,
		observation_type: ObservationType,
	) -> Result<(), Error> {
		if self.worthless.contains(&base) || self.worthless.contains(&quote) {
			return Ok(());
		}

		let b_amt = Amount::new(Quant::from_i128(1), &base);
		let q_amt = Amount::new(rate, &quote);

		self.add_equality(date, b_amt, q_amt, observation_type)
	}

	/// Adds a rate by passing two amounts of different currencies that are
	/// deemed to be identical in value to each other.
	pub fn add_equality(
		&mut self,
		date: Date,
		a: Amount,
		b: Amount,
		observation_type: ObservationType,
	) -> Result<(), Error> {
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
			return Ok(());
		}

		let graph = self
			.daily_graphs
			.entry(date)
			.or_insert_with(|| Graph::new(date));

		if graph
			.get_direct_rate(&a.currency, &b.currency, true)
			.is_some()
		{
			match observation_type {
				ObservationType::Declared => {
					bail!("Cannot declare multiple rates on same date")
				},
				ObservationType::Inferred => return Ok(()), // ignore this
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

		Ok(())
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
	) -> Result<(), Error> {
		let mut resolved = BTreeMap::new();

		for (date, graph) in &self.daily_graphs {
			if self.allow_warnings && graph.has_inconsistent_cycle() {
				println!("[{}]: currency conversion rates on this date are not internally consistent", date);
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

		for (base, quote, mut observation) in self.primary_graph.get_all_rates()
		{
			if let Some(precision) = determine_precision(
				max_precision_by_currency.get(&base),
				max_precision_by_currency.get(&quote),
			) {
				observation.rate.set_render_precision(precision, false);
			}

			// The primary graph is way more prone to drift because it uses
			// data irrespective of time, so it's only used when no other
			// rate is available.
			if !resolved.contains_key(&(base.clone(), quote.clone())) {
				resolved
					.entry((base.clone(), quote.clone()))
					.or_insert_with(Vec::new)
					.push(observation);
			}
		}

		self.resolved_rates = resolved;
		self.is_finalized = true;

		Ok(())
	}

	/// Retrieves the most recent rate, if any, at or before the given date
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
					.find(|o| o.date.is_none_or(|d| d <= *as_of))
					.map(|o| o.rate)
			})
	}

	/// Retrieves the most recent rate available, if any
	pub fn get_latest_rate(&self, base: &str, quote: &str) -> Option<Quant> {
		if !self.is_finalized {
			panic!("exchange rates not finalized")
		};

		self.resolved_rates
			.get(&(base.to_string(), quote.to_string()))
			.and_then(|rates| rates.first().map(|o| o.rate))
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

		assert!(exchange_rates
			.add_rate(
				date,
				base.clone(),
				quote.clone(),
				rate,
				ObservationType::Declared
			)
			.is_ok());

		let date2 = Date::from_str("2024-11-2").unwrap();
		assert!(exchange_rates
			.add_rate(
				date2,
				base,
				quote,
				Quant::new(12, 1),
				ObservationType::Declared
			)
			.is_ok());
	}

	#[test]
	fn test_declare_self_exchange() {
		let mut exchange_rates = ExchangeRates::new(false);
		let date = Date::from_str("2024-1-1").unwrap();
		let base = "USD".to_string();
		let rate = Quant::new(11, 1);

		assert!(exchange_rates
			.add_rate(
				date,
				base.clone(),
				base.clone(),
				rate,
				ObservationType::Declared
			)
			.is_err());

		let date2 = Date::from_str("2024-11-2").unwrap();
		assert!(exchange_rates
			.add_rate(
				date2,
				base.clone(),
				base,
				Quant::new(9, 1),
				ObservationType::Declared
			)
			.is_err());
	}

	#[test]
	fn test_declare_non_positive_rate() {
		let mut exchange_rates = ExchangeRates::new(false);
		let date = Date::from_str("2024-11-01").unwrap();
		let base = "USD".to_string();
		let quote = "EUR".to_string();

		assert!(exchange_rates
			.add_rate(
				date,
				base.clone(),
				quote.clone(),
				Quant::new(0, 0),
				ObservationType::Declared
			)
			.is_ok());
		assert!(exchange_rates
			.add_rate(
				date,
				base,
				quote,
				Quant::new(-1, 1),
				ObservationType::Declared
			)
			.is_ok());
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
		assert!(exchange_rates
			.add_rate(
				date,
				base.clone(),
				quote.clone(),
				inferred_rate,
				ObservationType::Inferred
			)
			.is_ok());

		let date2 = Date::from_str("2024-11-02").unwrap();
		assert!(exchange_rates
			.add_rate(
				date2,
				base.clone(),
				quote.clone(),
				Quant::new(111, 2),
				ObservationType::Inferred
			)
			.is_ok());
	}
}
