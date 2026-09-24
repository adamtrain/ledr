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

//! What the ledger already says, as a source of suggestions for new entries.
//!
//! Built from the lines as written, not the balanced ledger, so that a new
//! entry can be modelled on exactly what was typed last time.

use crate::input::fuzzy::{self, Score};
use crate::parsing::loader::Item;
use crate::syntax::lexer::{Directive, LineKind, Posting};
use crate::syntax::source::FileId;
use crate::util::date::Date;
use std::collections::{BTreeMap, BTreeSet};

/// How often and how recently something was used
#[derive(Clone, Debug, Default)]
pub struct Usage {
	pub count: usize,
	pub last: Date,
}

impl Usage {
	fn note(&mut self, date: Date) {
		self.count += 1;
		self.last = self.last.max(date);
	}

	/// A ranking bonus for being used often and recently
	pub fn weight(&self, today: Date) -> Score {
		let frequency = (self.count as f64 + 1.0).ln() * 25.0;
		let days = self.last.until(&today).total_days() as f64;
		let recency = 60.0 * (-days / 120.0).exp();
		(frequency + recency) as Score
	}
}

/// A description used before, and the entry it was last used on
#[derive(Clone, Debug)]
pub struct Payee {
	pub name: String,
	pub usage: Usage,
	/// The postings of the most recent entry with this description
	pub template: Vec<Posting>,
	/// When that entry was dated
	pub template_date: Date,
}

impl Payee {
	/// Whether the template can be reused by changing just an amount:
	/// simple postings, without prices or lots.
	pub fn is_simple(&self) -> bool {
		let mut currencies = self
			.template
			.iter()
			.filter_map(|p| p.amount.as_ref().map(|a| a.currency.as_str()));
		let first = currencies.next();
		self.template.iter().all(|p| {
			p.amount
				.as_ref()
				.is_none_or(|a| a.price.is_none() && a.lot.is_none())
		}) && currencies.all(|c| Some(c) == first)
	}
}

#[derive(Debug, Default)]
pub struct History {
	pub accounts: BTreeMap<String, Usage>,
	pub currencies: BTreeMap<String, Usage>,
	pub payees: BTreeMap<String, Payee>,
	pub declared_accounts: BTreeSet<String>,
	pub declared_currencies: BTreeSet<String>,
	/// For each account, how often each currency was used with it
	account_currencies: BTreeMap<String, BTreeMap<String, usize>>,
	/// For each account, how often each other account appeared beside it
	partners: BTreeMap<String, BTreeMap<String, usize>>,
	/// The file holding the latest-dated entry, and that date
	pub latest: Option<(FileId, Date)>,
	pub entries: usize,
}

struct Pending {
	date: Date,
	description: String,
	postings: Vec<Posting>,
	file: FileId,
}

impl History {
	pub fn build(items: &[Item]) -> Self {
		let mut history = Self::default();
		let mut pending: Option<Pending> = None;

		for item in items {
			let (file, line) = match item {
				Item::EndOfFile(_) => {
					history.finish(pending.take());
					continue;
				},
				Item::Line { file, line, .. } => (*file, line),
			};
			match &line.kind {
				LineKind::Blank => history.finish(pending.take()),
				LineKind::Header(header) => {
					history.finish(pending.take());
					pending = Some(Pending {
						date: header.date,
						description: header.description.clone(),
						postings: vec![],
						file,
					});
				},
				LineKind::Posting(posting) => {
					if let Some(p) = &mut pending {
						p.postings.push((**posting).clone());
					}
				},
				LineKind::Directive(d) => match &d.directive {
					Directive::Account(a) | Directive::Open(a) => {
						history.declared_accounts.insert(a.clone());
					},
					Directive::Currency(c) => {
						history.declared_currencies.insert(c.clone());
					},
					_ => {},
				},
				_ => {},
			}
		}
		history.finish(pending);
		history
	}

	fn finish(&mut self, entry: Option<Pending>) {
		let Some(entry) = entry else { return };
		if entry.postings.is_empty() {
			return;
		}
		self.entries += 1;

		if self.latest.is_none_or(|(_, date)| entry.date >= date) {
			self.latest = Some((entry.file, entry.date));
		}

		for posting in &entry.postings {
			self.accounts
				.entry(posting.account.clone())
				.or_default()
				.note(entry.date);
			if let Some(amount) = &posting.amount {
				self.currencies
					.entry(amount.currency.clone())
					.or_default()
					.note(entry.date);
				*self
					.account_currencies
					.entry(posting.account.clone())
					.or_default()
					.entry(amount.currency.clone())
					.or_default() += 1;
			}
			for other in &entry.postings {
				if other.account != posting.account {
					*self
						.partners
						.entry(posting.account.clone())
						.or_default()
						.entry(other.account.clone())
						.or_default() += 1;
				}
			}
		}

		let payee = self
			.payees
			.entry(entry.description.clone())
			.or_insert_with(|| Payee {
				name: entry.description.clone(),
				usage: Usage::default(),
				template: vec![],
				template_date: Date::min(),
			});
		payee.usage.note(entry.date);
		if entry.date >= payee.template_date {
			payee.template = entry.postings;
			payee.template_date = entry.date;
		}
	}

	/// Every account known, declared or used
	pub fn all_accounts(&self) -> BTreeSet<&str> {
		self.accounts
			.keys()
			.chain(&self.declared_accounts)
			.map(String::as_str)
			.collect()
	}

	/// Whether the ledger declares its accounts, so new ones need declaring
	pub fn uses_declarations(&self) -> bool {
		!self.declared_accounts.is_empty()
	}

	pub fn is_known_account(&self, account: &str) -> bool {
		self.accounts.contains_key(account)
			|| self.declared_accounts.contains(account)
	}

	pub fn is_known_currency(&self, currency: &str) -> bool {
		self.currencies.contains_key(currency)
			|| self.declared_currencies.contains(currency)
	}

	/// The currency most used overall
	pub fn main_currency(&self) -> Option<&str> {
		self.currencies
			.iter()
			.max_by_key(|(_, u)| u.count)
			.map(|(c, _)| c.as_str())
	}

	/// The currency most used with an account, else the main currency
	pub fn currency_for(&self, account: &str) -> Option<&str> {
		self.account_currencies
			.get(account)
			.and_then(|m| m.iter().max_by_key(|(_, n)| **n))
			.map(|(c, _)| c.as_str())
			.or_else(|| self.main_currency())
	}

	/// The account most often used alongside another
	pub fn usual_partner(&self, account: &str) -> Option<&str> {
		self.usual_partner_where(account, |_| true)
	}

	/// The account most often used alongside another, of those that pass
	/// a test
	pub fn usual_partner_where(
		&self,
		account: &str,
		test: impl Fn(&str) -> bool,
	) -> Option<&str> {
		self.partners
			.get(account)
			.and_then(|m| {
				m.iter().filter(|(a, _)| test(a)).max_by_key(|(_, n)| **n)
			})
			.map(|(a, _)| a.as_str())
	}

	/// Accounts matching a query, best first
	pub fn find_accounts(
		&self,
		query: &str,
		today: Date,
	) -> Vec<(&str, Score)> {
		let all: Vec<&str> = self.all_accounts().into_iter().collect();
		fuzzy::rank(
			query,
			all.iter(),
			|a| a,
			|a| self.accounts.get(*a).map_or(0, |u| u.weight(today)),
		)
		.into_iter()
		.map(|(a, s)| (*a, s))
		.collect()
	}

	/// Descriptions matching a query, best first
	pub fn find_payees(
		&self,
		query: &str,
		today: Date,
	) -> Vec<(&Payee, Score)> {
		fuzzy::rank(
			query,
			self.payees.values(),
			|p| &p.name,
			|p| p.usage.weight(today),
		)
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::parsing::loader::load;
	use crate::syntax::source::SourceMap;

	pub fn history(text: &str) -> History {
		let dir = tempfile::tempdir().unwrap();
		let path = dir.path().join("h.ledr");
		std::fs::write(&path, text).unwrap();
		let mut sources = SourceMap::new();
		let items = load(path.to_str().unwrap(), &mut sources, None).unwrap();
		History::build(&items)
	}

	const LEDGER: &str = "\
! 2024-01-01 account Assets:Checking
! 2024-01-01 account Liabilities:Visa

2024-01-02 Tim Hortons
    Expenses:Coffee   4.50 CAD
    Liabilities:Visa

2024-02-02 Tim Hortons
    Expenses:Coffee   5.25 CAD
    Liabilities:Visa

2024-01-15 Paycheck
    Assets:Checking   2,000.00 USD
    Income:Salary
";

	#[test]
	fn test_templates_come_from_the_latest_entry() {
		let h = history(LEDGER);
		let tim = &h.payees["Tim Hortons"];
		assert_eq!(tim.usage.count, 2);
		assert_eq!(tim.template_date.to_string(), "2024-02-02");
		assert_eq!(tim.template[0].amount.as_ref().unwrap().value_text, "5.25");
		assert!(tim.is_simple());
		assert_eq!(h.entries, 3);
	}

	#[test]
	fn test_usage() {
		let h = history(LEDGER);
		assert_eq!(h.currency_for("Expenses:Coffee"), Some("CAD"));
		assert_eq!(h.currency_for("Assets:Checking"), Some("USD"));
		assert_eq!(
			h.usual_partner("Expenses:Coffee"),
			Some("Liabilities:Visa")
		);
		assert!(h.uses_declarations());
		assert!(h.is_known_account("Income:Salary"));
		assert!(!h.is_known_account("Income:Bonus"));
		let today = Date::from_str("2024-03-01").unwrap();
		assert_eq!(h.find_accounts("cof", today)[0].0, "Expenses:Coffee");
		assert_eq!(h.find_payees("tim", today)[0].0.name, "Tim Hortons");
	}
}
