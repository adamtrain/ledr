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
use crate::config::config_file::Mercury;
use crate::gl::entry::Entry;
use crate::gl::ledger::Ledger;
use crate::import::account_guesser::AccountGuesser;
use crate::import::http::Client;
use crate::import::importer::PLACEHOLDER;
use crate::import::mercury::models::{
	Account, AccountParams, AccountTransactionsParams, AccountsHolder,
	Transaction, TransactionHolder,
};
use crate::util::amount::Amount;
use crate::util::date::Date;
use crate::util::quant::Quant;
use anyhow::{bail, Error};
use std::cmp::Ordering;
use std::collections::HashSet;
use std::fs::OpenOptions;
use std::io::Write;
use std::ops::Neg;

const MERCURY_API_URL: &str = "https://api.mercury.com/api/v1";

const ACCOUNT_PREFIX: &str = "Assets:US:Mercury";

/// The importer that knows how to contact the Mercury API to grab
/// transactions and write them as entries. Read-only implementation.
pub struct MercuryImporter {
	http: Client,

	/// Prefix used in references to Mercury asset accounts
	account_prefix: String,
	/// If set, ignore accounts other than this. Can match on
	/// account nickname or account number (not routing).
	account_filter: Option<String>,

	account_guesser: AccountGuesser,
}

impl MercuryImporter {
	pub fn new(config: Mercury, ledger: Ledger) -> Result<Self, Error> {
		if config.api_key.is_none() {
			bail!("no mercury api key in config");
		}

		let api_url = if let Some(url) = config.api_url {
			url
		} else {
			MERCURY_API_URL.to_owned()
		};

		Ok(MercuryImporter {
			http: Client::new(&api_url, config.api_key.unwrap()),
			account_prefix: config
				.account_prefix
				.unwrap_or(ACCOUNT_PREFIX.to_owned()),
			account_filter: config.account,
			account_guesser: AccountGuesser::new(ledger),
		})
	}

	pub fn run(
		&self,
		begin: Date,
		end: Date,
		file: String,
	) -> Result<(), Error> {
		// make sure we can append to destination file first
		let mut file =
			OpenOptions::new().append(true).create(true).open(file)?;

		let mut entries: Vec<Entry> = Vec::new();

		// hold unique transaction amounts & creation times to not count e.g.
		// internal transfers twice (b/c they show up in multiple txn lists)
		let mut internal_txn_holder: HashSet<(Quant, String)> = HashSet::new();

		// get accounts
		let resp: AccountsHolder =
			self.http.get("accounts", None::<AccountParams>)?;
		for account in &resp.accounts {
			if let Some(acct_filter) = &self.account_filter {
				if let Some(nickname) = &account.nickname {
					if nickname != acct_filter
						&& acct_filter != &account.account_number
					{
						continue;
					}
				} else if acct_filter != &account.account_number {
					continue;
				}
			}

			if account.typ != "mercury" || account.status != "active" {
				continue;
			}

			// get transactions within range
			let resp: TransactionHolder = self.http.get(
				format!("account/{}/transactions", account.id).as_str(),
				Some(AccountTransactionsParams {
					start: begin.to_string(),
					end: end.to_string(),
				}),
			)?;

			if resp.total >= 500 {
				// Partly to avoid flooding the user and partly to avoid dealing with
				// pagination, let's fail if the response for an account is above page size
				bail!("Too many transactions in range; please shorten range and try again");
			}

			entries.extend(resp.transactions.into_iter().filter_map(|txn| {
				let (amt, created_at) = (
					Quant::from_str(&txn.amount.clone()).unwrap().abs(),
					txn.created_at.clone(),
				);

				if txn.kind == "internalTransfer" {
					if internal_txn_holder.contains(&(amt, created_at.clone()))
					{
						return None;
					}
					internal_txn_holder.insert((amt, created_at));
				}

				self.parse_transaction(account, txn).unwrap_or(None)
			}));
		}

		entries.sort();
		for e in entries {
			writeln!(file, "{}", e)?;
		}

		Ok(())
	}

	fn parse_transaction(
		&self,
		a: &Account,
		t: Transaction,
	) -> Result<Option<Entry>, Error> {
		if t.status == "cancelled" || t.status == "failed" {
			return Ok(None);
		}

		let is_internal = t.kind == "internalTransfer";

		// Resolve entry name from whether it is internal, and resolve
		// counterparty based on whether we have seen the same one before.
		let (entry_name, counterparty) = if is_internal {
			(
				"Internal Transfer".to_string(),
				format!("{}:{}", self.account_prefix, t.name(true)),
			)
		} else if let Some(guessed) =
			self.account_guesser.lookup(&t.name(false))
		{
			println!(
				"[{}] using previously used account {} for counterparty {}",
				t.date(),
				guessed,
				t.name(false)
			);
			(t.name(false), guessed)
		} else {
			(t.name(false), PLACEHOLDER.to_string())
		};

		let mut entry = Entry::new(t.date(), entry_name, 0);

		let account_name = a.name(&self.account_prefix);

		let amount = Amount::new(Quant::from_str(&t.amount)?, "USD");
		let mut counterparty_amount = -amount.clone();

		// TODO: I do not have a way to test this until I spend CAD again soon
		if let Some(cei) = t.currency_exchange_info {
			match amount.value.cmp(&Quant::zero()) {
				Ordering::Greater => {
					// debit to account
					counterparty_amount = Amount::new(
						Quant::from_str(&cei.converted_from_amount)?,
						&cei.converted_from_currency,
					)
					.neg();
				},
				Ordering::Less => {
					// credit from account
					counterparty_amount = Amount::new(
						Quant::from_str(&cei.converted_to_amount)?,
						&cei.converted_to_currency,
					)
					.neg();
				},
				Ordering::Equal => return Ok(None),
			}
		}

		entry.add_detail(&account_name, amount)?;
		entry.add_detail(&counterparty, counterparty_amount)?;

		Ok(Some(entry))
	}
}
