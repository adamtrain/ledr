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
use crate::diagnostics::{Diagnostic, closest};
use crate::gl::declaration::Declaration;
use crate::gl::entry::{Entry, VIRTUAL_CONVERSION_ACCOUNT};
use crate::gl::exchange_rates::ExchangeRates;
use crate::gl::observed_rate::ObservationType;
use crate::investment::action::Action;
use crate::investment::action_buffer::ActionBuffer;
use crate::syntax::source::Span;
use crate::util::amount::Amount;
use crate::util::date::Date;
use anyhow::{Error, bail};
use std::collections::BTreeMap;

/// The only valid top-level account names. This is an accounting system, after
/// all! Society has rules! Granted, there is no functional reason to have this
/// requirement other than sorting guarantees when presenting reports.
///
/// If you are reading this and want a variant of this for a language other than
/// English, email me with the right terms to use for each category, and I will
/// implement a parallel one for your language.
pub const VALID_PREFIXES: [&str; 5] =
	["Assets", "Liabilities", "Equity", "Income", "Expenses"];

/// Checks that an account's first segment is one of the valid prefixes.
pub fn check_account_prefix(account: &str) -> Result<(), Diagnostic> {
	let top = account.split(':').next().unwrap_or_default();
	if VALID_PREFIXES.contains(&top) {
		return Ok(());
	}
	let error = Diagnostic::error(format!(
		"`{account}` must start with one of {}",
		VALID_PREFIXES.join(", ")
	));
	Err(match closest(top, VALID_PREFIXES) {
		Some(prefix) => error
			.help(format!("did you mean `{prefix}{}`?", &account[top.len()..])),
		None => error,
	})
}

/// The central data structure of this system that takes input from the parser
/// and assembles it into accounting journal entries. Entries have detail lines
/// and details contain or imply various metadata, like exchange rates, lot
/// activity, and so on.
///
/// The Ledger is chiefly responsible for assembling this data according to
/// input. In general, the Ledger will pass one or more of its data sets to
/// another data structure for further refinement and reporting.
#[derive(Debug)]
pub struct Ledger {
	entries: Vec<Entry>,
	/// Entry currently being assembled, if any
	pending_entry: Option<Entry>,

	/// Skip currency and account validation steps
	lenient_mode: bool,

	/// currency -> the earliest date currency is allowed to appear
	declared_currencies: BTreeMap<String, Date>,
	/// account -> the earliest date account is allowed to appear
	declared_accounts: BTreeMap<String, Declaration>,
	/// currency + the date prior to which we should ignore this currency
	declared_clears: Vec<(String, Date)>,

	// conceptually distinct modules the ledger must populate or access
	pub exchange_rates: ExchangeRates,
	pub lots: ActionBuffer,

	/// Things that are allowed but look like mistakes
	pub warnings: Vec<Diagnostic>,
}

impl Ledger {
	/// A `thorough` ledger runs expensive consistency checks as well.
	pub fn new(lenient: bool, thorough: bool) -> Self {
		Self {
			entries: vec![],
			pending_entry: None,
			lenient_mode: lenient,
			declared_currencies: Default::default(),
			declared_accounts: Default::default(),
			declared_clears: Default::default(),
			exchange_rates: ExchangeRates::new(thorough),
			lots: Default::default(),
			warnings: vec![],
		}
	}

	pub fn is_lenient(&self) -> bool {
		self.lenient_mode
	}

	// -----------
	// -- INPUT --
	// -----------

	pub fn declare_currency(
		&mut self,
		currency: &str,
		date: Date,
	) -> Result<(), Error> {
		if self.lenient_mode {
			return Ok(());
		}

		if self.declared_currencies.contains_key(currency) {
			bail!("Currency {currency} declared twice")
		}

		self.declared_currencies.insert(currency.to_string(), date);

		Ok(())
	}

	pub fn declare_account(
		&mut self,
		account: String,
		date: Date,
	) -> Result<(), Error> {
		check_account_prefix(&account)?;
		if self.lenient_mode {
			return Ok(());
		}

		if self.declared_accounts.contains_key(&account) {
			bail!("Account {account} declared twice")
		}

		let mut declaration = Declaration::new();
		declaration.open_account(date)?;

		self.declared_accounts.insert(account.clone(), declaration);

		Ok(())
	}

	/// Declares to the ledger that details related to the given
	/// currency on or prior to the given date should be ignored
	/// for the purposes of reporting.
	///
	/// The intention is to hide old instruments that you never
	/// want to see again from reports, without having to delete
	/// records of history.
	pub fn declare_clear(&mut self, currency: String, date: Date) {
		self.declared_clears.push((currency, date));
	}

	/// Reopens a closed account. If the account was not closed, this isn't
	/// ideal syntax, but it's alright to use "open" to declare an account
	/// initially, I think. The reverse does not hold; you cannot use
	/// "account" to reopen a closed account.
	pub fn declare_account_open(
		&mut self,
		account: String,
		date: Date,
	) -> Result<(), Error> {
		check_account_prefix(&account)?;
		self.declared_accounts
			.entry(account.clone())
			.or_insert_with(Declaration::new)
			.open_account(date)
			.or_else(|_| self.declare_account(account, date))
	}

	pub fn declare_account_closure(
		&mut self,
		account: String,
		date: Date,
	) -> Result<(), Error> {
		check_account_prefix(&account)?;
		self.declared_accounts
			.entry(account)
			.or_insert_with(Declaration::new)
			.close_account(date)
	}

	pub fn has_pending_entry(&self) -> bool {
		self.pending_entry.is_some()
	}

	pub fn new_entry(
		&mut self,
		date: Date,
		desc: String,
		index: usize,
	) -> Result<(), Error> {
		if self.pending_entry.is_some() {
			self.finish_entry()?;
		}

		self.pending_entry = Some(Entry::new(date, desc, index));
		Ok(())
	}

	/// Records that a line of source belongs to the entry being assembled,
	/// so that problems with the entry can point at all of it.
	pub fn extend_pending_span(&mut self, line: Span) {
		if let Some(entry) = &mut self.pending_entry {
			entry.extend_span(line);
		}
	}

	/// Adds a line to the pending entry. Whether its account and currencies
	/// were declared is the caller's business; see [`Ledger::check_account`]
	/// and [`Ledger::check_currency`].
	pub fn add_detail(
		&mut self,
		account: String,
		amount: Amount,
		inline_conversion: Option<Amount>,
		cost_basis: Option<Amount>,
		lot_name: Option<String>,
	) -> Result<(), Error> {
		if self.pending_entry.is_none() {
			bail!("Orphaned entry detail")
		}

		if account.is_empty() {
			bail!("Account is empty")
		}
		check_account_prefix(&account)?;

		let pending_entry = self.pending_entry.as_mut().unwrap();
		pending_entry.add_detail(&account, amount.clone())?;

		if let Some(ica) = inline_conversion {
			// An inline conversion has the authority of a
			// declaration in many ways, but in case there are
			// multiple intraday transactions that differ from each
			// other (as day traders etc. experience all the time),
			// we must treat them as inferred rates here.
			let conflict = self.exchange_rates.add_rate(
				*pending_entry.get_date(),
				amount.currency.clone(),
				ica.currency.clone(),
				ica.value,
				ObservationType::Inferred,
			)?;
			if let Some(conflict) = conflict {
				self.warnings
					.push(pending_entry.rate_conflict_warning(&conflict));
			}

			// Move the imbalance to the cost basis currency via
			// the virtual conversion account, if this is not a lot
			if cost_basis.is_none() {
				let conversion = VIRTUAL_CONVERSION_ACCOUNT.to_string();
				pending_entry.add_system_detail(
					&conversion,
					Amount::new(ica.value * amount.value, &ica.currency),
				)?;
				pending_entry
					.add_system_detail(&conversion, -amount.clone())?;
			}
		}

		if let Some(cb) = cost_basis {
			pending_entry.add_action(Action::new(
				*pending_entry.get_date(),
				account.clone(),
				amount,
				cb,
				lot_name,
			)?);
		}

		Ok(())
	}

	/// Sets the account name of the entry with no accompanying balance or
	/// currency. This account is then assumed to be the counterparty to all
	/// other detail lines in the entry that remain with any imbalance at
	/// the end of processing.
	///
	/// In most cases, setting this forces an entry to balance, one way or
	/// another.
	pub fn set_virtual_detail(&mut self, account: String) -> Result<(), Error> {
		if self.pending_entry.is_none() {
			bail!("Orphaned entry detail")
		}

		check_account_prefix(&account)?;

		self.pending_entry
			.as_mut()
			.unwrap()
			.set_virtual_detail(account)
	}

	/// Adds a reference line, which does nothing except appear on some reports
	pub fn add_reference(&mut self, reference: String) -> Result<(), Error> {
		match &mut self.pending_entry {
			Some(e) => {
				e.add_reference(reference);
				Ok(())
			},
			None => bail!("Orphaned reference"),
		}
	}

	/// Takes the pending entry, finalizes it, processes its lots, and adds
	/// it to the set of entries on this. No-ops if nothing is pending.
	pub fn finish_entry(&mut self) -> Result<(), Error> {
		match self.pending_entry.take() {
			None => Ok(()),
			Some(mut entry) => {
				if entry.details().is_empty() {
					return Err(Diagnostic::error(format!(
						"Entry `{}` has no postings",
						entry.get_desc()
					))
					.at_opt(entry.span())
					.help(
						"an entry needs at least one line with an account and amount",
					)
					.into());
				}

				let actions = entry
					.finalize(&mut self.exchange_rates, &mut self.warnings)
					.map_err(|e| match entry.span() {
						Some(span) => Diagnostic::locate(e, span),
						None => e,
					})?;
				for action in actions {
					self.lots.add_action(action);
				}

				self.entries.push(entry);
				Ok(())
			},
		}
	}

	/// Checks whether a currency has been declared for use, and checks the
	/// pending entry to make sure the declaration date is not ahead of the
	/// pending entry where the currency appears.
	pub fn check_currency(&self, currency: &str) -> Result<(), Error> {
		let Some(declaration_date) = self.declared_currencies.get(currency)
		else {
			let error = Diagnostic::error(format!(
				"Currency {currency} used without declaration"
			));
			let known = self.declared_currencies.keys().map(String::as_str);
			return Err(match closest(currency, known) {
				Some(suggestion) => {
					error.help(format!("did you mean {suggestion}?"))
				},
				None => error.help(format!(
					"declare it with `! {} currency {currency}`, or run with --lenient",
					self.pending_date()
				)),
			}
			.into());
		};

		if &self.pending_date() < declaration_date {
			bail!(
				"Currency {currency} used prior to declaration on {declaration_date}"
			)
		}

		Ok(())
	}

	/// Checks whether an account has been declared for use, and checks the
	/// pending entry to make sure the declaration date is not ahead of the
	/// pending entry where the account appears.
	pub fn check_account(&self, account: &str) -> Result<(), Error> {
		let Some(declaration) = self.declared_accounts.get(account) else {
			let error = Diagnostic::error(format!(
				"Account {account} used without declaration"
			));
			let known = self.declared_accounts.keys().map(String::as_str);
			return Err(match closest(account, known) {
				Some(suggestion) => {
					error.help(format!("did you mean {suggestion}?"))
				},
				None => error.help(format!(
					"declare it with `! {} account {account}`, or run with --lenient",
					self.pending_date()
				)),
			}
			.into());
		};

		if !declaration.is_open_on(&self.pending_date()) {
			bail!("Account {account} is not open on {}", self.pending_date())
		}

		Ok(())
	}

	pub fn is_account_declared(&self, account: &str) -> bool {
		self.declared_accounts.contains_key(account)
	}

	pub fn is_currency_declared(&self, currency: &str) -> bool {
		self.declared_currencies.contains_key(currency)
	}

	/// The date of the entry being assembled, or the latest possible date
	fn pending_date(&self) -> Date {
		self.pending_entry
			.as_ref()
			.map_or(Date::max(), |e| *e.get_date())
	}

	// ----------------
	// -- TABULATING --
	// ----------------

	/// Finalizes the entire ledger by standardizing the visible precision of
	/// each currency, and dropping entries prior to the start date. Note that
	/// dropping entries in this way has no effect on observed exchange rates
	/// or their effects. There is no exposed way to drop after a given date
	/// because the parser takes care of that, to make sure all end dates that
	/// are passed by the user are definitively point-in-time accurate.
	///
	/// At this stage, we round off amounts that do not fit in the desired
	/// precision, and we put those rounded off amounts, if any, into the
	/// rounding error equity account.
	pub fn finalize(
		&mut self,
		drop_before: &Date,
		max_reso_by_currency: &BTreeMap<String, u32>,
		overall_max_reso: Option<u32>,
	) -> Result<(), Error> {
		let max_reso = overall_max_reso.unwrap_or(u32::MAX);

		self.entries.retain(|e| e.get_date() >= drop_before);

		// A clear directive says to ignore a given currency prior to a given
		// date, so we iterate through those and remove details as required.
		// By this point, all currencies must independently balance within each
		// entry, and all details on an entry share a date, so this cannot
		// impact the overall ledger balance.
		for (currency, date) in &mut self.declared_clears {
			self.entries.retain_mut(|e| {
				if e.get_date() <= date {
					let should_drop = e.remove_currency(currency);
					!should_drop
				} else {
					true
				}
			})
		}

		self.entries.sort();
		for entry in &mut self.entries {
			for (currency, &reso) in max_reso_by_currency {
				entry
					.set_precision_for_currency(currency, reso.min(max_reso))?;
			}

			entry.reduce(vec![VIRTUAL_CONVERSION_ACCOUNT])
		}

		Ok(())
	}

	pub fn entries(&self) -> &Vec<Entry> {
		&self.entries
	}

	pub fn take_entries(self) -> Vec<Entry> {
		self.entries
	}

	/// Every account the ledger knows of, declared or used
	pub fn declared_accounts(&self) -> impl Iterator<Item = &str> {
		self.declared_accounts.keys().map(String::as_str)
	}

	/// Every currency the ledger has declared
	pub fn declared_currencies(&self) -> impl Iterator<Item = &str> {
		self.declared_currencies.keys().map(String::as_str)
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::util::date::Date;
	use crate::util::quant::Quant;

	#[test]
	fn test_ledger_initialization() {
		let ledger = Ledger::new(true, true);
		assert!(ledger.entries.is_empty());
		assert!(ledger.pending_entry.is_none());
		assert!(ledger.declared_currencies.is_empty());
		assert!(ledger.declared_accounts.is_empty());
		assert!(ledger.lenient_mode);
	}

	#[test]
	fn test_declare_currency() {
		let mut ledger = Ledger::new(false, false);
		let date = Date::from_str("2024-1-1").unwrap();

		assert!(ledger.declare_currency("USD", date).is_ok());
		assert!(ledger.declared_currencies.contains_key("USD"));

		assert!(ledger.declare_currency("USD", date).is_err());
	}

	#[test]
	fn test_declare_account() {
		let mut ledger = Ledger::new(false, false);
		let date = Date::from_str("2024-01-01").unwrap();

		assert!(
			ledger
				.declare_account("Assets:Cash".to_string(), date)
				.is_ok()
		);
		assert!(ledger.declared_accounts.contains_key("Assets:Cash"));

		assert!(
			ledger
				.declare_account("Assets:Cash".to_string(), date)
				.is_err()
		);
	}

	#[test]
	fn test_new_entry() {
		let mut ledger = Ledger::new(false, false);
		let date = Date::from_str("2024-01-01").unwrap();

		assert!(ledger.new_entry(date, "Test Entry".to_string(), 0).is_ok());
		assert!(ledger.pending_entry.is_some());
		assert_eq!(ledger.pending_entry.unwrap().get_date(), &date);
	}

	#[test]
	fn test_add_detail_valid() {
		let mut ledger = Ledger::new(false, false);
		let date = Date::from_str("2024-01-01").unwrap();
		ledger.declare_currency("USD", date).unwrap();
		ledger
			.declare_account("Assets:Cash".to_string(), date)
			.unwrap();

		ledger.new_entry(date, "Test Entry".to_string(), 0).unwrap();

		assert!(
			ledger
				.add_detail(
					"Assets:Cash".to_string(),
					Amount::new(Quant::new(1000, 1), "USD",),
					None,
					None,
					None,
				)
				.is_ok()
		);
	}

	#[test]
	fn test_check_currency_undeclared() {
		let mut ledger = Ledger::new(false, false);
		let date = Date::from_str("2024-01-01").unwrap();
		ledger.declare_currency("USD", date).unwrap();

		ledger.new_entry(date, "Test Entry".to_string(), 0).unwrap();

		assert!(ledger.check_currency("EUR").is_err());
		let error = ledger.check_currency("usd").unwrap_err();
		let diagnostic = error.downcast::<Diagnostic>().unwrap();
		assert_eq!(diagnostic.help.unwrap(), "did you mean USD?");
	}

	#[test]
	fn test_check_account_undeclared() {
		let mut ledger = Ledger::new(false, false);
		let date = Date::from_str("2024-01-01").unwrap();
		ledger
			.declare_account("Liabilities:Loans".to_string(), date)
			.unwrap();

		ledger.new_entry(date, "Test Entry".to_string(), 0).unwrap();

		let error = ledger.check_account("Liabilities:Loan").unwrap_err();
		let diagnostic = error.downcast::<Diagnostic>().unwrap();
		assert_eq!(diagnostic.help.unwrap(), "did you mean Liabilities:Loans?");
	}

	#[test]
	fn test_invalid_prefix_suggests_a_fix() {
		let error = check_account_prefix("Asset:Cash").unwrap_err();
		assert_eq!(error.help.unwrap(), "did you mean `Assets:Cash`?");
		assert!(check_account_prefix("AssetsX:Cash").is_err());
		assert!(check_account_prefix("Assets").is_ok());
	}

	#[test]
	fn test_add_detail_orphaned_entry() {
		let mut ledger = Ledger::new(false, false);

		assert!(
			ledger
				.add_detail(
					"Assets:Cash".to_string(),
					Amount::new(Quant::new(1000, 1), "USD",),
					None,
					None,
					None
				)
				.is_err()
		);
	}

	#[test]
	fn test_finish_entry() {
		let mut ledger = Ledger::new(false, false);
		let date = Date::from_str("2024-01-01").unwrap();
		ledger.new_entry(date, "Test Entry".to_string(), 0).unwrap();
		let mut dec = Declaration::new();
		dec.open_account(date).unwrap();
		ledger
			.declared_accounts
			.insert("Assets:Cash".to_string(), dec);
		ledger.declared_currencies.insert("USD".to_string(), date);
		ledger
			.add_detail(
				"Assets:Cash".to_string(),
				Amount::new(Quant::new(1000, 1), "USD"),
				None,
				None,
				None,
			)
			.unwrap();
		ledger
			.add_detail(
				"Assets:Cash".to_string(),
				Amount::new(Quant::new(-1000, 1), "USD"),
				None,
				None,
				None,
			)
			.unwrap();
		assert!(ledger.finish_entry().is_ok());
		assert!(ledger.pending_entry.is_none());
		assert_eq!(ledger.entries.len(), 1);
	}

	#[test]
	fn test_check_currency_before_declaration() {
		let mut ledger = Ledger::new(false, false);
		let date = Date::from_str("2024-01-01").unwrap();
		ledger
			.declare_currency("USD", Date::from_str("2024-1-2").unwrap())
			.unwrap();

		ledger.new_entry(date, "Test Entry".to_string(), 0).unwrap();
		let result = ledger.check_currency("USD");

		assert!(result.is_err());
	}

	#[test]
	fn test_check_account_before_declaration() {
		let mut ledger = Ledger::new(false, false);
		let date = Date::from_str("2024-01-01").unwrap();
		ledger
			.declare_account(
				"Assets:Cash".to_string(),
				Date::from_str("2024-01-02").unwrap(),
			)
			.unwrap();

		ledger.new_entry(date, "Test Entry".to_string(), 0).unwrap();
		let result = ledger.check_account("Assets:Cash");

		assert!(result.is_err());
	}

	#[test]
	fn test_add_detail_without_currency_declaration_in_lenient_mode() {
		let mut ledger = Ledger::new(true, false);
		let date = Date::from_str("2024-01-01").unwrap();

		ledger
			.new_entry(date, "Lenient Test Entry".to_string(), 0)
			.unwrap();

		assert!(
			ledger
				.add_detail(
					"Assets:Cash".to_string(),
					Amount::new(Quant::new(500, 1), "EUR",),
					None,
					None,
					None
				)
				.is_ok()
		);
	}

	#[test]
	fn test_add_detail_without_account_declaration_in_lenient_mode() {
		let mut ledger = Ledger::new(true, false);
		let date = Date::from_str("2024-01-01").unwrap();

		ledger
			.new_entry(date, "Lenient Test Entry".to_string(), 0)
			.unwrap();

		assert!(
			ledger
				.add_detail(
					"Liabilities:Loan".to_string(),
					Amount::new(Quant::new(1000, 1), "USD",),
					None,
					None,
					None
				)
				.is_ok()
		);
	}

	#[test]
	fn test_set_virtual_detail_without_account_declaration_in_lenient_mode() {
		let mut ledger = Ledger::new(true, false);
		let date = Date::from_str("2024-01-01").unwrap();

		ledger
			.new_entry(date, "Lenient Virtual Detail Test".to_string(), 0)
			.unwrap();

		assert!(
			ledger
				.set_virtual_detail("Equity:OpeningBalance".to_string())
				.is_ok()
		);
	}
}
