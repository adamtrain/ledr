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
use crate::gl::exchange_rates::ExchangeRates;
use crate::gl::observed_rate::ObservationType;
use crate::investment::action::{Action, Direction};
use crate::util::amount::Amount;
use crate::util::date::Date;
use crate::util::quant::Quant;
use anyhow::{bail, Error};
use std::cmp::Ordering;
use std::collections::BTreeMap;
use std::string::ToString;

pub(crate) const VIRTUAL_CONVERSION_ACCOUNT: &str = "Equity:Conversions";

#[derive(Clone, Debug, Eq)]
pub struct Entry {
	date: Date,

	/// The position in the ledger of this, relative to other Entry objects.
	/// Used to properly order these on some reports.
	index: usize,

	desc: String,
	details: Vec<Detail>,

	virtual_detail: Option<String>,

	reference: Option<String>, // optional string, not inspected

	/// Lot actions related to this entry. Don't read until finalization,
	/// because we need to associate proceeds with sales, if known, which
	/// requires context across multiple detail lines.
	actions: Vec<Action>,
}

impl Entry {
	pub fn new(date: Date, desc: String, index: usize) -> Self {
		Self {
			date,
			index,
			desc,
			details: vec![],
			virtual_detail: None,
			reference: None,
			actions: vec![],
		}
	}

	pub fn add_detail(
		&mut self,
		account: &str,
		amount: Amount,
	) -> Result<(), Error> {
		if account.is_empty() {
			bail!("Account is empty")
		}

		self.details.push(Detail::new(account, amount, false));

		Ok(())
	}

	pub fn add_system_detail(
		&mut self,
		account: &str,
		amount: Amount,
	) -> Result<(), Error> {
		if account.is_empty() {
			bail!("Account is empty")
		}

		self.details.push(Detail::new(account, amount, true));

		Ok(())
	}

	pub fn add_action(&mut self, action: Action) {
		self.actions.push(action);
	}

	pub fn set_virtual_detail(&mut self, account: String) -> Result<(), Error> {
		if self.virtual_detail.is_some() {
			bail!("Only one line per entry may omit amount and currency")
		}

		if account.is_empty() {
			bail!("Account is empty")
		}

		self.virtual_detail = Some(account);
		Ok(())
	}

	/// Adds a reference to the entry, to be used for human purposes.
	/// The system is guaranteed not to analyze it or use it for any reason
	/// or purpose. Some reports and queries will display it, however.
	///
	/// If a note is already present, it appends the two, separated by one
	/// newline character.
	pub fn add_reference(&mut self, reference: String) {
		match &mut self.reference {
			Some(existing_note) => {
				existing_note.push('\n');
				existing_note.push_str(reference.trim());
			},
			None => {
				self.reference = Some(reference.trim().to_string());
			},
		}
	}

	pub fn get_desc(&self) -> &String {
		&self.desc
	}

	pub fn get_date(&self) -> &Date {
		&self.date
	}

	pub fn get_reference(&self) -> String {
		if let Some(reference) = &self.reference {
			reference.clone()
		} else {
			"".to_string()
		}
	}

	pub fn details(&self) -> &Vec<Detail> {
		&self.details
	}

	/// Returns the net amount from this entry on the given account, i.e.
	/// the sum of all detail lines related to the account, for all
	/// currencies. Accounts will match on any substring of their name.
	pub fn net_for_account(&self, account: &String) -> BTreeMap<String, Quant> {
		let mut net_map = BTreeMap::new();

		for detail in &self.details {
			if !&detail.account.contains(account) {
				continue;
			}

			*net_map
				.entry(detail.currency().to_string())
				.or_insert(Quant::zero()) += detail.value();
		}

		net_map
	}

	/// Sets render precision for all details of the given currency.
	/// No effect on underlying value.
	pub fn set_precision_for_currency(
		&mut self,
		currency: &str,
		decimal_places: u32,
	) -> Result<(), Error> {
		for detail in &mut self.details {
			if detail.amount.currency == *currency {
				detail
					.amount
					.value
					.set_render_precision(decimal_places, true);
			}
		}

		Ok(())
	}

	/// Removes all Details denominated in the given currency. Should only
	/// be used on finalized, balanced entries, else balance results could
	/// get weird.
	///
	/// If all details were removed this way, reports true, indicating that
	/// the caller should drop this entry entirely.
	pub fn remove_currency(&mut self, currency: &str) -> bool {
		self.details.retain(|detail| detail.currency() != currency);
		self.details.is_empty()
	}

	/// Completes an entry. We have to pass the exchange rate set in here,
	/// because this is where exchange rates are inferred in some cases,
	/// i.e. if exactly two currencies are imbalanced.
	///
	/// Returns the final set of actions related to any lots that moved
	/// due to this entry.
	pub fn finalize(
		&mut self,
		rates: &mut ExchangeRates,
		allow_warnings: bool,
	) -> Result<Vec<Action>, Error> {
		let actual_details = self.get_actual_details();
		let actions = self.actions.clone();

		// Special case if exactly one line with a lot exists and is
		// netted against a virtual detail, in which case it is implied
		// that the cost basis of the lot should be netted against the
		// virtual detail rather than the asset held or sold
		if actions.len() == 1
			&& actual_details.len() == 1
			&& self.virtual_detail.is_some()
		{
			let actual_detail = actual_details.first().unwrap();
			let action = actions.first().unwrap();

			self.add_system_detail(
				VIRTUAL_CONVERSION_ACCOUNT,
				-actual_detail.amount.clone(),
			)?;
			self.add_system_detail(
				VIRTUAL_CONVERSION_ACCOUNT,
				Amount::new(
					action.commodity.cost_basis().value * actual_detail.value(),
					&action.commodity.cost_basis().currency,
				),
			)?;
		}

		let mut imbalances = self.get_imbalances();

		// Special case if exactly two currencies are unbalanced with no
		// virtual account, in which case we net them against each other
		if imbalances.len() == 2 && self.virtual_detail.is_none() {
			self.multiline_implicit_currency_conversion(
				&mut imbalances,
				rates,
				allow_warnings,
			)?;
		}

		// Attach proceeds to associated sell actions if possible
		self.resolve_sell_action_proceeds(actual_details, allow_warnings);

		// If a virtual detail exists, it can absorb all imbalances.
		// Otherwise, if any remain, we fail the entry as unbalanced.
		while let Some((currency, value)) = imbalances.pop() {
			if let Some(vd) = &self.virtual_detail {
				self.details.push(Detail::new(
					vd,
					Amount::new(-value, &currency),
					true,
				));
			} else {
				bail!("Unbalanced entry")
			}
		}

		Ok(self.actions.clone())
	}

	/// This is a special case in which there is no virtual detail, but
	/// there are exactly two lines that we can net against each other.
	/// Because we allow negative exchange rates between currencies to
	/// account for things like shorts, we do not check if the two values
	/// are cardinally opposed.
	fn multiline_implicit_currency_conversion(
		&mut self,
		imbalances: &mut Vec<(String, Quant)>,
		rates: &mut ExchangeRates,
		allow_warnings: bool,
	) -> Result<(), Error> {
		let (currency1, amount1) = imbalances.remove(0);
		let (currency2, amount2) = imbalances.remove(0);

		if allow_warnings
			&& ((amount1 > 0 && amount2 > 0) || (amount1 < 0 && amount2 < 0))
		{
			println!("[{} {}] entry implies a negative exchange rate (is this intentional?)",
			self.date, self.desc)
		}

		self.add_system_detail(
			VIRTUAL_CONVERSION_ACCOUNT,
			Amount::new(-amount1, &currency1),
		)?;
		self.add_system_detail(
			VIRTUAL_CONVERSION_ACCOUNT,
			Amount::new(-amount2, &currency2),
		)?;

		// This implies an exchange rate between the currencies
		rates.add_equality(
			self.date,
			Amount::new(amount1, &currency1),
			Amount::new(-amount2, &currency2),
			ObservationType::Inferred,
		)?;

		Ok(())
	}

	/// Adds proceeds to applicable Sell lot actions iff they can be
	/// inferred from user input.
	fn resolve_sell_action_proceeds(
		&mut self,
		actual_details: Vec<Detail>,
		allow_warnings: bool,
	) {
		let actions_copy = self.actions.clone();
		for sale in &mut self.actions {
			if sale.direction == Direction::Buy {
				continue;
			}

			if actual_details.len() > 2
				|| (actual_details.len() == 2 && self.virtual_detail.is_some())
			{
				if allow_warnings {
					println!(
						"[{} {}] entry has ambiguous proceeds from lot sale",
						self.date, self.desc
					);
				}
				return;
			}

			if self.virtual_detail.is_some() {
				// Perfectly netted out against the cost basis
				if allow_warnings {
					println!(
						"[{} {}] entry has lot sale netted against nonspecific \
						detail, implying break-even (is this intentional?)",
						self.date, self.desc
					);
				}
				sale.add_unit_proceeds(sale.commodity.cost_basis().clone());
				return;
			}

			// If the only other Detail is a buy action, net against its cost
			// basis, but log a warning if allowed because it can be ambiguous
			if let Some(other_action) = actions_copy.iter().find(|a| *a != sale)
			{
				println!(
					"[{} {}] entry marks the purchase of a lot as the \
						 proceeds of another (consider separating these)",
					self.date, self.desc,
				);
				let other_quantity = match &other_action.direction {
					Direction::Buy => other_action.quantity,
					Direction::Sell(_) => -other_action.quantity,
				};

				sale.add_unit_proceeds(Amount::new(
					other_quantity * other_action.commodity.cost_basis().value
						/ sale.quantity,
					&other_action.commodity.cost_basis().currency,
				));
			} else {
				// The normal case, where the other Detail is a normal line,
				// so we associate it as the proceeds of the sale of this lot
				let detail_opt = actual_details.iter().find(|&a| {
					a.amount.currency != sale.commodity.symbol()
						|| a.amount.value != -sale.quantity
				});
				if let Some(detail) = detail_opt {
					sale.add_unit_proceeds(Amount::new(
						detail.value() / sale.quantity,
						detail.currency(),
					));
				}
			}
		}
	}

	/// Rebuilds the detail set to remove system details that cancel each
	/// other out in the given accounts. Entries in other accounts are kept
	/// unchanged, but in actuality, the whole detail set is reallocated.
	/// Ideally, only call this once, for the sake of efficiency.
	pub fn reduce(&mut self, accounts: Vec<&str>) {
		let mut all_other_details: Vec<_> = self
			.details
			.iter()
			.filter(|&d| !d.is_system || !accounts.contains(&&*d.account))
			.cloned()
			.collect();

		for account in accounts.iter() {
			let system_details: Vec<_> = self
				.details
				.iter()
				.filter(|d| d.is_system && *account == d.account)
				.collect();

			let mut balances_by_currency: BTreeMap<String, Quant> =
				BTreeMap::new();
			for detail in system_details {
				*balances_by_currency
					.entry(detail.currency().to_string())
					.or_insert(Quant::zero()) += detail.value();
			}

			let reduced_details: Vec<Detail> = balances_by_currency
				.iter()
				.filter_map(|(currency, balance)| {
					if *balance != 0 {
						Some(Detail::new(
							account,
							Amount::new(*balance, currency),
							true,
						))
					} else {
						None
					}
				})
				.collect();
			all_other_details.extend(reduced_details);
		}

		self.details = all_other_details;
	}

	/// Find all currencies that don't sum to zero, with amounts
	fn get_imbalances(&self) -> Vec<(String, Quant)> {
		let mut balances: BTreeMap<String, Quant> = BTreeMap::new();

		// Sum up the values for each currency
		for detail in &self.details {
			*balances
				.entry(detail.currency().to_string())
				.or_insert(Quant::zero()) += detail.value();
		}

		// Filter for currencies that don't sum to zero
		balances
			.into_iter()
			.filter(|&(_, value)| value != Quant::zero())
			.collect()
	}

	/// Returns those details that were not inserted automatically
	fn get_actual_details(&self) -> Vec<Detail> {
		self.details
			.iter()
			.filter(|&d| !d.is_system)
			.cloned()
			.collect()
	}

	/// Helper designed for use with the importer. If the entry has exactly
	/// two line items, one of them is an Asset account, and the other is not,
	/// returns the other Detail that is not an Asset account. Else None.
	///
	/// Ignores system-created details in this procedure.
	pub fn get_counterparty_detail(&self) -> Option<Detail> {
		let details = self.get_actual_details();

		if details.len() != 2 {
			return None;
		}

		let asset_detail =
			details.iter().find(|d| d.account.starts_with("Assets:"));

		if let Some(asset) = asset_detail {
			details.iter().find(|d| d.account != asset.account).cloned()
		} else {
			None
		}
	}
}

use std::fmt;

impl fmt::Display for Entry {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		// Render the header line with date and description
		writeln!(f, "{} {}", self.date, self.desc)?;

		// Render the reference lines, word-wrapping at 80 characters overall
		if let Some(r) = &self.reference {
			let wrapped_lines = wrap_text(r, 69);
			for line in wrapped_lines {
				writeln!(f, "\t// {}", line)?;
			}
		}

		if self.details.is_empty() {
			return Ok(());
		}

		// Calculate the maximum width of the account strings
		let account_width = self
			.details
			.iter()
			.map(|d| d.account.len())
			.max()
			.unwrap_or(0);

		// Align amounts and currencies properly
		let mut detail_lines = Vec::new();
		let mut max_value_width = 0;
		for detail in &self.details {
			let value_str = format!("{}", detail.value());
			max_value_width = max_value_width.max(value_str.len());
			detail_lines.push((
				detail.account.clone(),
				value_str,
				detail.currency().clone(),
			));
		}

		// Format the detail lines with proper alignment
		for (account, value, currency) in detail_lines {
			writeln!(
				f,
				"\t{:<account_width$}  {:>value_width$} {}",
				account,
				value,
				currency,
				account_width = account_width,
				value_width = max_value_width
			)?;
		}

		Ok(())
	}
}

/// Helper function to wrap text into lines of specified width
fn wrap_text(text: &str, max_width: usize) -> Vec<String> {
	let mut lines = Vec::new();
	let mut current_line = String::new();

	for word in text.split_whitespace() {
		if !current_line.is_empty()
			&& current_line.len() + word.len() + 1 > max_width
		{
			lines.push(current_line);
			current_line = String::new();
		}
		if !current_line.is_empty() {
			current_line.push(' ');
		}
		current_line.push_str(word);
	}

	if !current_line.is_empty() {
		lines.push(current_line);
	}

	lines
}

impl PartialEq for Entry {
	fn eq(&self, other: &Self) -> bool {
		self.date == other.date && self.desc == other.desc
	}
}

// Entry Ord and PartialOrd Implementation
impl Ord for Entry {
	fn cmp(&self, other: &Self) -> Ordering {
		self.date
			.cmp(&other.date)
			.then_with(|| self.index.cmp(&other.index))
			.then_with(|| self.desc.cmp(&other.desc))
			.then_with(|| self.details.len().cmp(&other.details.len()))
			.then_with(|| self.actions.len().cmp(&other.actions.len()))
	}
}

impl PartialOrd for Entry {
	fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
		Some(self.cmp(other))
	}
}

/// A specific line item in an Entry, indicating a credit or debit and the
/// associated account.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Detail {
	account: String,
	amount: Amount,
	/// True iff the system inserted this detail and it did not come from
	/// user input
	is_system: bool,
}

impl Detail {
	pub fn new(account: &str, amount: Amount, is_system: bool) -> Self {
		Self {
			account: account.to_string(),
			amount,
			is_system,
		}
	}

	pub fn account(&self) -> &String {
		&self.account
	}

	pub fn currency(&self) -> &String {
		&self.amount.currency
	}

	pub fn value(&self) -> Quant {
		self.amount.value
	}
}

// Detail Ord and PartialOrd Implementation
impl Ord for Detail {
	fn cmp(&self, other: &Self) -> Ordering {
		self.account
			.cmp(&other.account)
			.then_with(|| self.amount.currency.cmp(&other.amount.currency))
			.then_with(|| self.amount.value.cmp(&other.amount.value))
			.then_with(|| self.is_system.cmp(&other.is_system))
	}
}

impl PartialOrd for Detail {
	fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
		Some(self.cmp(other))
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::gl::exchange_rates::ExchangeRates;
	use crate::util::date::Date;
	use crate::util::quant::Quant;

	fn create_entry() -> Entry {
		Entry::new(
			Date::from_str("2024-1-1").unwrap(),
			"Sample Entry".to_string(),
			0,
		)
	}

	#[test]
	fn test_entry_creation() {
		let entry = create_entry();
		assert_eq!(entry.get_date(), &Date::from_str("2024-1-1").unwrap());
		assert!(entry.details.is_empty());
	}

	#[test]
	fn test_add_detail() {
		let mut entry = create_entry();
		let result = entry
			.add_detail("Assets:Cash", Amount::new(Quant::new(1000, 1), "USD"));

		assert!(result.is_ok());
		assert_eq!(entry.details.len(), 1);

		let detail = &entry.details[0];
		assert_eq!(detail.account, "Assets:Cash");
		assert_eq!(detail.amount.value, Quant::new(1000, 1));
		assert_eq!(detail.amount.currency, "USD");
	}

	#[test]
	fn test_add_detail_empty_account() {
		let mut entry = create_entry();
		let result =
			entry.add_detail("", Amount::new(Quant::new(1000, 1), "USD"));

		assert!(result.is_err());
	}

	#[test]
	fn test_finalize_unbalanced_entry() {
		let mut entry = create_entry();
		entry
			.add_detail("Assets:Cash", Amount::new(Quant::new(1000, 1), "USD"))
			.unwrap();
		entry
			.add_detail(
				"Expenses:Food",
				Amount::new(Quant::new(-500, 1), "USD"),
			)
			.unwrap();

		let mut rates = ExchangeRates::new(false);
		let result = entry.finalize(&mut rates, false);

		assert!(result.is_err());
	}

	#[test]
	fn test_finalize_balanced_entry() {
		let mut entry = create_entry();
		entry
			.add_detail("Assets:Cash", Amount::new(Quant::new(1000, 1), "USD"))
			.unwrap();
		entry
			.add_detail(
				"Expenses:Food",
				Amount::new(Quant::new(-1000, 1), "USD"),
			)
			.unwrap();

		let mut rates = ExchangeRates::new(false);
		let result = entry.finalize(&mut rates, false);

		assert!(result.is_ok());
	}

	#[test]
	fn test_set_virtual_detail() {
		let mut entry = create_entry();
		let result = entry.set_virtual_detail("Assets:Virtual".to_string());

		assert!(result.is_ok());
		assert!(entry.virtual_detail.is_some());
		assert_eq!(entry.virtual_detail.unwrap(), "Assets:Virtual")
	}

	#[test]
	fn test_set_virtual_detail_twice() {
		let mut entry = create_entry();
		entry
			.set_virtual_detail("Assets:Virtual".to_string())
			.unwrap();
		let result = entry.set_virtual_detail("Assets:Another".to_string());

		assert!(result.is_err());
	}

	#[test]
	fn test_set_virtual_detail_empty_account() {
		let mut entry = create_entry();
		let result = entry.set_virtual_detail("".to_string());

		assert!(result.is_err());
	}

	#[test]
	fn test_multiline_implicit_currency_conversion() {
		let mut entry = create_entry();
		entry
			.add_detail("Assets:Cash", Amount::new(Quant::new(1000, 1), "USD"))
			.unwrap();
		entry
			.add_detail("Assets:Bank", Amount::new(Quant::new(-2000, 1), "EUR"))
			.unwrap();

		let mut rates = ExchangeRates::new(false);
		let mut imbalances = entry.get_imbalances();
		let result = entry.multiline_implicit_currency_conversion(
			&mut imbalances,
			&mut rates,
			false,
		);

		assert!(result.is_ok());
		assert_eq!(entry.details.len(), 4);
	}

	#[test]
	fn test_reduce_removes_canceling_system_details() {
		let mut entry = create_entry();

		entry.details = vec![
			Detail::new(
				"account1",
				Amount::new(Quant::from_i128(100), "USD"),
				true,
			),
			Detail::new(
				"account1",
				Amount::new(Quant::from_i128(-100), "USD"),
				true,
			),
		];

		entry.reduce(vec!["account1"]);

		assert!(entry.details.is_empty());
	}

	#[test]
	fn test_reduce_keeps_non_canceling_details() {
		let mut entry = create_entry();

		entry.details = vec![
			Detail::new(
				"account1",
				Amount::new(Quant::from_i128(100), "USD"),
				true,
			),
			Detail::new(
				"account1",
				Amount::new(Quant::from_i128(50), "USD"),
				true,
			),
		];

		entry.reduce(vec!["account1"]);

		assert_eq!(entry.details.len(), 1);
		assert_eq!(entry.details[0].account, "account1");
		assert_eq!(entry.details[0].amount.value, 150);
		assert_eq!(entry.details[0].amount.currency, "USD");
	}

	#[test]
	fn test_reduce_ignores_non_system_details() {
		let mut entry = create_entry();

		entry.details = vec![
			Detail::new(
				"account1",
				Amount::new(Quant::from_i128(100), "USD"),
				false,
			),
			Detail::new(
				"account1",
				Amount::new(Quant::from_i128(-100), "USD"),
				true,
			),
		];

		entry.reduce(vec!["account1"]);

		assert_eq!(entry.details.len(), 2);
	}

	#[test]
	fn test_reduce_preserves_other_account_details() {
		let mut entry = create_entry();

		entry.details = vec![
			Detail::new(
				"account1",
				Amount::new(Quant::from_i128(100), "USD"),
				true,
			),
			Detail::new(
				"account2",
				Amount::new(Quant::from_i128(200), "USD"),
				true,
			),
		];

		entry.reduce(vec!["account1"]);

		assert_eq!(entry.details.len(), 2);

		assert_eq!(entry.details[0].account, "account2");
		assert_eq!(entry.details[0].amount.currency, "USD");
		assert_eq!(entry.details[1].account, "account1");
		assert_eq!(entry.details[1].amount.currency, "USD");
	}

	#[test]
	fn test_reduce_handles_multiple_currencies() {
		let mut entry = create_entry();

		entry.details = vec![
			Detail::new(
				"account1",
				Amount::new(Quant::from_i128(100), "USD"),
				true,
			),
			Detail::new(
				"account1",
				Amount::new(Quant::from_i128(200), "EUR"),
				true,
			),
			Detail::new(
				"account1",
				Amount::new(Quant::from_i128(-200), "GBP"),
				true,
			),
		];

		entry.reduce(vec!["account1"]);

		assert_eq!(entry.details.len(), 3);
	}

	#[test]
	fn test_get_counterparty_detail_with_two_details() {
		let mut entry = create_entry();
		entry
			.add_detail("Assets:Cash", Amount::new(Quant::new(1000, 1), "USD"))
			.unwrap();
		entry
			.add_detail(
				"Income:Salary",
				Amount::new(Quant::new(-1000, 1), "USD"),
			)
			.unwrap();

		let counterparty = entry.get_counterparty_detail();

		assert!(counterparty.is_some());
		let counterparty = counterparty.unwrap();
		assert_eq!(counterparty.account, "Income:Salary");
		assert_eq!(counterparty.amount.value, Quant::new(-1000, 1));
		assert_eq!(counterparty.amount.currency, "USD");
	}

	#[test]
	fn test_get_counterparty_detail_with_no_asset_account() {
		let mut entry = create_entry();
		entry
			.add_detail(
				"Expenses:Food",
				Amount::new(Quant::new(-500, 1), "USD"),
			)
			.unwrap();
		entry
			.add_detail("Income:Bonus", Amount::new(Quant::new(500, 1), "USD"))
			.unwrap();

		let counterparty = entry.get_counterparty_detail();

		assert!(counterparty.is_none());
	}

	#[test]
	fn test_get_counterparty_detail_with_more_than_two_details() {
		let mut entry = create_entry();
		entry
			.add_detail("Assets:Cash", Amount::new(Quant::new(1000, 1), "USD"))
			.unwrap();
		entry
			.add_detail(
				"Income:Salary",
				Amount::new(Quant::new(-1000, 1), "USD"),
			)
			.unwrap();
		entry
			.add_detail(
				"Expenses:Taxes",
				Amount::new(Quant::new(-200, 1), "USD"),
			)
			.unwrap();

		let counterparty = entry.get_counterparty_detail();

		assert!(counterparty.is_none());
	}

	#[test]
	fn test_get_counterparty_detail_with_one_detail() {
		let mut entry = create_entry();
		entry
			.add_detail("Assets:Bank", Amount::new(Quant::new(1000, 1), "USD"))
			.unwrap();

		let counterparty = entry.get_counterparty_detail();

		assert!(counterparty.is_none());
	}
}
