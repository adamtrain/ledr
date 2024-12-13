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

const VIRTUAL_ROUNDING_ERROR_ACCOUNT: &str = "Equity:Rounding";

use crate::gl::entry::Detail;
use crate::gl::exchange_rates::ExchangeRates;
use crate::gl::ledger::Ledger;
use crate::util::amount::Amount;
use crate::util::quant::Quant;
use std::collections::BTreeMap;

/// Each total represents one account or segment, one position on the hierarchy,
/// that may have a balance. For example, for the ledger with hierarchy:
///
/// Assets
///      Cash
///      AR
/// Liabilities
///      Short-Term
///      Long-Term
///
/// Each of these lines would have a Total object. The Assets and Liabilities
/// totals would each have subtotal lists of length 2.
///
/// There is a top level total which will always have amount values of 0 in each
/// currency, because double-entry accounting, and account string "". The only
/// time the top level will be nonzero is after filtering.
#[derive(Debug, Default)]
pub struct Total {
	pub account: String,
	pub subtotals: BTreeMap<String, Total>, // account name -> next total
	pub depth: u32, // top level total is depth 0; Income/Expenses is 1, etc.

	/// Note that a Total does not need to be last in the chain
	/// to have amounts on it. These amounts do not include those
	/// on leaves below.
	amounts: BTreeMap<String, Quant>, // currency -> balance held
}

impl Total {
	pub fn new() -> Self {
		Default::default()
	}

	/// Recursively calculates the total amounts for this Total and all its
	/// subtotals, and returns the result. This is an abstraction that allows
	/// any Total in the hierarchy to report the sum of itself and any Totals
	/// below it.
	pub fn amounts(&self) -> BTreeMap<String, Quant> {
		let mut aggregated_amounts: BTreeMap<String, Quant> =
			self.amounts.clone();

		for subtotal in self.subtotals.values() {
			for (currency, amount) in subtotal.amounts() {
				aggregated_amounts
					.entry(currency)
					.and_modify(|e| *e += amount)
					.or_insert(amount);
			}
		}

		aggregated_amounts
	}

	pub fn from_ledger(ledger: &Ledger) -> Self {
		let mut total = Self::new();

		let all_details: Vec<Detail> = ledger
			.entries()
			.iter()
			.flat_map(|e| e.details().iter().cloned())
			.collect();

		total.ingest_details(&all_details);
		total
	}

	pub fn ingest_details(&mut self, details: &Vec<Detail>) {
		for detail in details {
			let mut current = &mut *self;

			for segment in detail.account().split(":").collect::<Vec<&str>>() {
				current = current
					.subtotals
					.entry(segment.to_string())
					.or_insert_with(|| Total {
						account: segment.to_string(),
						amounts: BTreeMap::new(),
						subtotals: BTreeMap::new(),
						depth: current.depth + 1,
					});
			}

			// Update the leaf node with the final amount
			*current
				.amounts
				.entry(detail.currency().to_string())
				.or_insert_with(Quant::zero) += detail.value();
		}
	}

	// -------------
	// -- FILTERS --
	// -------------

	/// Drops those subtotals not matching the given strs vec, then sums all
	/// subtotals by currency and updates top-level totals with them.
	/// Designed for filtering to a subset of the VALID_PREFIXES.
	pub fn filter_top_level(&mut self, strs: Vec<&str>) {
		self.subtotals
			.retain(|name, _| strs.contains(&name.as_str()));

		let mut currency_totals: BTreeMap<String, Quant> = BTreeMap::new();

		// Sum subtotals; doesn't need to be recursive because we only
		// dropped some top-level branches of the hierarchy; what
		// remains is accurate
		for subtotal in self.subtotals.values_mut() {
			for (currency, amount) in &subtotal.amounts {
				currency_totals
					.entry(currency.clone())
					.and_modify(|e| *e += *amount)
					.or_insert_with(|| *amount);
			}
		}

		self.amounts = currency_totals.into_iter().collect();
	}

	/// Designed for use with currency reports, we convert all totals to
	/// the target currency. If not possible, currencies are dropped iff
	/// the given bool is marked true.
	pub fn collapse_to(
		&mut self,
		currency: &String,
		exchange_rates: &mut ExchangeRates,
		ignore_non_convertable_balances: bool,
	) {
		let mut converted_total = Quant::zero();
		let mut retained_balances = BTreeMap::new();

		for (c, balance) in &self.amounts {
			if c == currency {
				converted_total += *balance;
			} else if let Some(rate) =
				exchange_rates.get_latest_rate(c, currency)
			{
				converted_total += *balance * rate;
			} else if !ignore_non_convertable_balances {
				retained_balances.insert(c.clone(), *balance);
			}
		}

		self.amounts.clear();
		self.amounts.insert(currency.clone(), converted_total);
		if !ignore_non_convertable_balances {
			self.amounts.extend(retained_balances);
		}

		// Recursively collapse subtotals
		for subtotal in self.subtotals.values_mut() {
			subtotal.collapse_to(
				currency,
				exchange_rates,
				ignore_non_convertable_balances,
			);
		}
	}

	/// Invert the signs of every Quant in the hierarchy
	pub fn invert(&mut self) {
		for quant in self.amounts.values_mut() {
			quant.negate();
		}

		for subtotal in self.subtotals.values_mut() {
			subtotal.invert();
		}
	}

	/// Rounds all totals in this and its hierarchy to the given precision.
	/// The precision will be either the overall max precision, or the highest
	/// precision observed in the ledger for its currency, whichever is lower.
	///
	/// Banker's rounding is used for all rounding operations.
	///
	/// This procedure may introduce rounding error if the precision for any
	/// given balance is too low to represent the actual total. In such
	/// cases, cumulative rounding error is reported to the user as a virtual
	/// posting to Equity:Rounding, provided it impacts the final report at
	/// the requested precision.
	pub fn round(
		&mut self,
		max_precision: u32,
		max_reso_by_currency: &mut BTreeMap<String, u32>,
		is_top_level: bool,
	) {
		// Round off amounts on this
		for (currency, quant) in self.amounts.iter_mut() {
			let reso = max_reso_by_currency
				.get(currency.as_str())
				.unwrap_or(&max_precision);

			quant.round(*reso.min(&max_precision));
		}

		// Recursively round down the stack of any subtotals this has.
		for subtotal in self.subtotals.values_mut() {
			subtotal.round(max_precision, max_reso_by_currency, false);
		}

		// If rounding error has propagated to the top-level, then
		// the only recourse is to book a rounding error entry.
		if is_top_level {
			for (currency, quant) in &self.amounts() {
				if *quant == 0 {
					continue;
				}

				self.ingest_details(&vec![Detail::new(
					VIRTUAL_ROUNDING_ERROR_ACCOUNT,
					Amount::new(-*quant, currency),
					true,
				)])
			}
		}
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::gl::entry::Detail;
	use crate::util::amount::Amount;
	use crate::util::quant::Quant;

	#[test]
	fn test_total_initialization() {
		let total = Total::new();
		assert_eq!(total.account, "");
		assert_eq!(total.amounts.len(), 0);
		assert_eq!(total.subtotals.len(), 0);
		assert_eq!(total.depth, 0);
	}

	#[test]
	fn test_ingest_details_single_detail() {
		let mut total = Total::new();
		let detail = Detail::new(
			"Assets:Cash",
			Amount::new(Quant::new(1000, 1), "USD"),
			false,
		);
		total.ingest_details(&vec![detail]);

		assert_eq!(total.subtotals.len(), 1);
		assert!(total.subtotals.contains_key("Assets"));
		assert_eq!(total.subtotals["Assets"].subtotals.len(), 1);
		assert!(total.subtotals["Assets"].subtotals.contains_key("Cash"));

		let cash_total = &total.subtotals["Assets"].subtotals["Cash"];
		assert_eq!(cash_total.amounts.get("USD"), Some(&Quant::new(1000, 1)));
	}

	#[test]
	fn test_ingest_details_multiple_details_same_currency() {
		let mut total = Total::new();
		let details = vec![
			Detail::new(
				"Assets:Cash",
				Amount::new(Quant::new(1000, 1), "USD"),
				false,
			),
			Detail::new(
				"Assets:AR",
				Amount::new(Quant::new(2000, 1), "USD"),
				false,
			),
		];
		total.ingest_details(&details);

		assert_eq!(total.subtotals.len(), 1);
		assert!(total.subtotals.contains_key("Assets"));

		let assets_total = &total.subtotals["Assets"];
		assert_eq!(assets_total.subtotals.len(), 2);
		assert!(assets_total.subtotals.contains_key("Cash"));
		assert!(assets_total.subtotals.contains_key("AR"));

		let cash_total = &assets_total.subtotals["Cash"];
		let ar_total = &assets_total.subtotals["AR"];
		assert_eq!(cash_total.amounts.get("USD"), Some(&Quant::new(1000, 1)));
		assert_eq!(ar_total.amounts.get("USD"), Some(&Quant::new(2000, 1)));
	}

	#[test]
	fn test_ingest_details_hierarchy() {
		let mut total = Total::new();
		let detail = Detail::new(
			"Liabilities:Short-Term:CreditCard",
			Amount::new(Quant::new(500, 1), "EUR"),
			false,
		);
		total.ingest_details(&vec![detail]);

		assert_eq!(total.subtotals.len(), 1);
		assert!(total.subtotals.contains_key("Liabilities"));

		let liabilities_total = &total.subtotals["Liabilities"];
		assert!(liabilities_total.subtotals.contains_key("Short-Term"));

		let short_term_total = &liabilities_total.subtotals["Short-Term"];
		assert!(short_term_total.subtotals.contains_key("CreditCard"));

		let credit_card_total = &short_term_total.subtotals["CreditCard"];
		assert_eq!(
			credit_card_total.amounts.get("EUR"),
			Some(&Quant::new(500, 1))
		);
	}

	#[test]
	fn test_filter_top_level() {
		let mut total = Total::new();
		total.ingest_details(&vec![
			Detail::new(
				"Assets:Cash",
				Amount::new(Quant::new(1000, 1), "USD"),
				false,
			),
			Detail::new(
				"Liabilities:CreditCard",
				Amount::new(Quant::new(500, 1), "USD"),
				false,
			),
		]);

		total.filter_top_level(vec!["Assets"]);
		assert_eq!(total.subtotals.len(), 1);
		assert!(total.subtotals.contains_key("Assets"));
		assert!(!total.subtotals.contains_key("Liabilities"));

		let assets_total = &total.subtotals["Assets"];
		assert_eq!(
			assets_total.amounts().get("USD"),
			Some(&Quant::new(1000, 1))
		);
	}

	#[test]
	fn test_invert() {
		let mut total = Total::new();
		total.ingest_details(&vec![
			Detail::new(
				"Income:Sales",
				Amount::new(Quant::new(3000, 1), "USD"),
				false,
			),
			Detail::new(
				"Expenses:Rent",
				Amount::new(Quant::new(1000, 1), "USD"),
				false,
			),
		]);

		total.invert();

		let sales_total = &total.subtotals["Income"].subtotals["Sales"];
		let rent_total = &total.subtotals["Expenses"].subtotals["Rent"];

		assert_eq!(sales_total.amounts.get("USD"), Some(&Quant::new(-3000, 1)));
		assert_eq!(rent_total.amounts.get("USD"), Some(&Quant::new(-1000, 1)));
	}

	#[test]
	fn test_no_subtotals_in_empty_total() {
		let total = Total::new();
		assert!(total.subtotals.is_empty());
	}

	#[test]
	fn test_filter_top_level_empty_filter() {
		let mut total = Total::new();
		total.ingest_details(&vec![
			Detail::new(
				"Assets:Cash",
				Amount::new(Quant::new(1000, 1), "USD"),
				false,
			),
			Detail::new(
				"Liabilities:CreditCard",
				Amount::new(Quant::new(500, 1), "USD"),
				false,
			),
		]);

		total.filter_top_level(vec![]);
		assert!(total.subtotals.is_empty());
	}
}
