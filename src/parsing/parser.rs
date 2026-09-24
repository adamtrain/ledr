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

//! Builds a [`Ledger`] from lexed lines.

use crate::diagnostics::{Diagnostic, Locate};
use crate::gl::ledger::Ledger;
use crate::gl::observed_rate::ObservationType;
use crate::parsing::loader::Item;
use crate::syntax::lexer::{Cols, Directive, LineKind, Posting};
use crate::syntax::source::{FileId, Span};
use crate::util::amount::Amount;
use crate::util::date::Date;
use anyhow::Error;
use std::collections::{BTreeMap, HashSet};

#[derive(Debug, Default)]
pub struct ParseResult {
	/// Greatest amount of precision indicated for each currency
	pub max_precision_by_currency: BTreeMap<String, u32>,
	/// Latest date of any entry (ignores directives)
	pub latest_date: Date,
	/// Problems found while carrying on past them, if asked to
	pub problems: Vec<Diagnostic>,
	/// Accounts used without being declared, with the first date used
	pub undeclared_accounts: BTreeMap<String, Date>,
	/// Currencies used without being declared, with the first date used
	pub undeclared_currencies: BTreeMap<String, Date>,
}

impl ParseResult {
	fn note_precision(&mut self, currency: &str, precision: u32) {
		let entry = self
			.max_precision_by_currency
			.entry(currency.to_owned())
			.or_insert(0);
		*entry = (*entry).max(precision);
	}

	fn note_date(&mut self, date: Date) {
		if self.latest_date < date {
			self.latest_date = date;
		}
	}
}

/// Assembles the ledger from every line, as read by the loader, in two
/// passes. The first applies directives and the second everything else, so
/// directives may appear anywhere, even after the entries that rely on them.
/// Anything dated after `ignore_after` is skipped.
///
/// With `keep_going`, problems with declarations and unbalanced entries are
/// collected into the result rather than stopping the build, so that they
/// can all be reported at once.
pub fn build(
	items: &[Item],
	ledger: &mut Ledger,
	ignore_after: &Date,
	keep_going: bool,
) -> Result<ParseResult, Error> {
	let mut builder = Builder {
		ledger,
		result: ParseResult::default(),
		keep_going,
		reported: HashSet::new(),
		pending_date: None,
	};
	builder.directives(items, ignore_after)?;
	builder.entries(items, ignore_after)?;
	Ok(builder.result)
}

fn span(file: FileId, number: usize, cols: Cols) -> Span {
	Span::new(file, number, cols.start, cols.end)
}

struct Builder<'a> {
	ledger: &'a mut Ledger,
	result: ParseResult,
	keep_going: bool,
	/// Messages already collected, so repeats are not reported twice
	reported: HashSet<String>,
	/// The date of the entry being built, if any
	pending_date: Option<Date>,
}

impl Builder<'_> {
	/// A failure that stops the build, unless we are keeping going, in which
	/// case it is noted and the build carries on
	fn soft(
		&mut self,
		outcome: Result<(), Error>,
		at: Option<Span>,
	) -> Result<(), Error> {
		let Err(error) = outcome else {
			return Ok(());
		};
		let error = match at {
			Some(at) => Diagnostic::locate(error, at),
			None => error,
		};
		if !self.keep_going {
			return Err(error);
		}
		let diagnostic = match error.downcast::<Diagnostic>() {
			Ok(d) => d,
			Err(other) => Diagnostic::error(other.to_string()).at_opt(at),
		};
		if self.reported.insert(diagnostic.message.clone()) {
			self.result.problems.push(diagnostic);
		}
		Ok(())
	}

	fn finish(&mut self) -> Result<(), Error> {
		let outcome = self.ledger.finish_entry();
		self.soft(outcome, None)
	}

	fn check_account(&mut self, account: &str, at: Span) -> Result<(), Error> {
		if self.ledger.is_lenient() {
			return Ok(());
		}
		if !self.ledger.is_account_declared(account) {
			let date = self.ledger_date();
			self.result
				.undeclared_accounts
				.entry(account.to_string())
				.or_insert(date);
		}
		let outcome = self.ledger.check_account(account);
		self.soft(outcome, Some(at))
	}

	fn check_currency(
		&mut self,
		currency: &str,
		at: Span,
	) -> Result<(), Error> {
		if self.ledger.is_lenient() {
			return Ok(());
		}
		if !self.ledger.is_currency_declared(currency) {
			let date = self.ledger_date();
			self.result
				.undeclared_currencies
				.entry(currency.to_string())
				.or_insert(date);
		}
		let outcome = self.ledger.check_currency(currency);
		self.soft(outcome, Some(at))
	}

	/// The date of the entry being built
	fn ledger_date(&self) -> Date {
		self.pending_date.unwrap_or_default()
	}

	fn directives(
		&mut self,
		items: &[Item],
		ignore_after: &Date,
	) -> Result<(), Error> {
		for item in items {
			let Item::Line { file, number, line } = item else {
				continue;
			};
			let LineKind::Directive(d) = &line.kind else {
				continue;
			};
			if &d.date > ignore_after {
				continue;
			}

			let at = span(*file, *number, d.args);
			let date = d.date;
			let ledger = &mut *self.ledger;
			let outcome = match &d.directive {
				Directive::Account(account) => {
					ledger.declare_account(account.clone(), date)
				},
				Directive::Open(account) => {
					ledger.declare_account_open(account.clone(), date)
				},
				Directive::Close(account) => {
					ledger.declare_account_closure(account.clone(), date)
				},
				Directive::Currency(currency) => {
					ledger.declare_currency(currency, date)
				},
				Directive::Clear(currency) => {
					ledger.declare_clear(currency.clone(), date);
					Ok(())
				},
				Directive::Worthless(currency) => {
					ledger.exchange_rates.declare_worthless(currency.clone());
					Ok(())
				},
				Directive::Rate {
					base, quote, rate, ..
				} => ledger
					.exchange_rates
					.add_rate(
						date,
						base.clone(),
						quote.clone(),
						*rate,
						ObservationType::Declared,
					)
					.map(|_| ()),
			};
			self.soft(outcome, Some(at))?;
		}
		Ok(())
	}

	fn entries(
		&mut self,
		items: &[Item],
		ignore_after: &Date,
	) -> Result<(), Error> {
		// Entries dated after the end of the range are skipped along with
		// everything that belongs to them
		let mut skipping = false;
		// Kept to sort entries on some reports in the order they appear,
		// second only to their date
		let mut entry_count = 0;

		for item in items {
			let (file, number, line) = match item {
				Item::EndOfFile(_) => {
					self.finish()?;
					continue;
				},
				Item::Line { file, number, line } => (*file, *number, line),
			};
			let whole_line = Span::new(file, number, 0, 0);

			match &line.kind {
				LineKind::Blank => self.finish()?,
				LineKind::CommentOnly
				| LineKind::Include(_)
				| LineKind::Directive(_) => {},
				LineKind::Reference(reference) => {
					if skipping {
						continue;
					}
					if !self.ledger.has_pending_entry() {
						return Err(
							outside_entry("Reference", whole_line).into()
						);
					}
					self.ledger.extend_pending_span(whole_line);
					if !reference.is_empty() {
						self.ledger.add_reference(reference.clone())?;
					}
				},
				LineKind::Header(header) => {
					self.finish()?;
					skipping = &header.date > ignore_after;
					if skipping {
						continue;
					}
					self.ledger
						.new_entry(
							header.date,
							header.description.clone(),
							entry_count,
						)
						.locate(whole_line)?;
					self.pending_date = Some(header.date);
					self.ledger.extend_pending_span(whole_line);
					entry_count += 1;
					self.result.note_date(header.date);
				},
				LineKind::Posting(posting) => {
					if skipping {
						continue;
					}
					if !self.ledger.has_pending_entry() {
						return Err(outside_entry("Posting", whole_line).into());
					}
					self.ledger.extend_pending_span(whole_line);
					self.posting(posting, file, number)?;
				},
			}
		}

		self.finish()
	}

	fn posting(
		&mut self,
		posting: &Posting,
		file: FileId,
		number: usize,
	) -> Result<(), Error> {
		let account = posting.account.clone();
		let account_at = span(file, number, posting.account_cols);
		let line_at = Span::new(file, number, 0, 0);

		self.check_account(&account, account_at)?;

		let Some(a) = &posting.amount else {
			return self.ledger.set_virtual_detail(account).locate(account_at);
		};

		self.check_currency(&a.currency, span(file, number, a.cols))?;
		let amount = Amount::new(a.value, &a.currency);
		self.result
			.note_precision(&a.currency, a.value.render_precision());

		if let Some(lot) = &a.lot {
			self.result
				.note_precision(&lot.currency, lot.cost.render_precision());
			self.check_currency(&lot.currency, span(file, number, lot.cols))?;

			// The purchase of a lot implies an exchange rate for that lot on
			// that date, with its cost basis. The sale of a lot does not.
			let cost_basis = Amount::new(lot.cost, &lot.currency);
			let implied_conversion = (a.value > 0).then(|| cost_basis.clone());
			return self
				.ledger
				.add_detail(
					account,
					amount,
					implied_conversion,
					Some(cost_basis),
					lot.name.clone(),
				)
				.locate(line_at);
		}

		if let Some(price) = &a.price {
			self.result.note_precision(
				&price.currency,
				price.value.render_precision(),
			);
			self.check_currency(
				&price.currency,
				span(file, number, price.cols),
			)?;

			// A total price is for the whole amount, whichever way it moves
			let unit_price = if price.is_total {
				price.value / a.value.abs()
			} else {
				price.value
			};
			return self
				.ledger
				.add_detail(
					account,
					amount,
					Some(Amount::new(unit_price, &price.currency)),
					None,
					None,
				)
				.locate(line_at);
		}

		self.ledger
			.add_detail(account, amount, None, None, None)
			.locate(line_at)
	}
}

fn outside_entry(what: &str, at: Span) -> Diagnostic {
	Diagnostic::error(format!("{what} outside of an entry"))
		.at(at)
		.help(
			"entries begin with a date and description, and a blank line ends them",
		)
}
