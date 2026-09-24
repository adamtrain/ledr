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

//! Adding an entry from a few words, like `ledr add coffee 4.50`.
//!
//! The words are sorted into a date, an amount, account hints (`@visa`) and
//! a description. The description is matched against past entries, whose
//! accounts are reused, so that most entries need only a name and a number.

use crate::diagnostics::Diagnostic;
use crate::gl::ledger::check_account_prefix;
use crate::input::compose::{Draft, DraftPosting};
use crate::input::expr::{self, Typed};
use crate::input::fuzzy::{self, INITIALS, SEGMENT, SUBSTRING};
use crate::input::history::{History, Payee};
use crate::util::date::Date;
use chrono::Datelike;

/// The words of a quick entry, sorted by what they are
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Quick {
	pub date: Option<Date>,
	pub amount: Option<Typed>,
	/// Account hints, from `@word` or anything that looks like an account
	pub hints: Vec<String>,
	/// Everything else, taken as the description
	pub description: String,
}

const WEEKDAYS: [&str; 7] = [
	"monday",
	"tuesday",
	"wednesday",
	"thursday",
	"friday",
	"saturday",
	"sunday",
];

/// A day as people say it: `today`, `yesterday`, a weekday (the most recent
/// one, counting today), `03-15`, `2024-03-15`, or, if `offsets` is set,
/// `-3` for three days ago.
pub fn parse_day(text: &str, today: Date, offsets: bool) -> Option<Date> {
	let text = text.trim().to_lowercase();
	match text.as_str() {
		"today" | "t" => return Some(today),
		"yesterday" | "y" => return Some(today.add_days(-1)),
		"tomorrow" => return Some(today.add_days(1)),
		_ => {},
	}
	if text.len() >= 3
		&& let Some(index) = WEEKDAYS.iter().position(|w| w.starts_with(&text))
	{
		let current = today.to_naive().weekday().num_days_from_monday() as i64;
		let back = (current - index as i64).rem_euclid(7);
		return Some(today.add_days(-back));
	}
	if offsets
		&& let Some(days) =
			text.strip_prefix('-').and_then(|d| d.parse::<i64>().ok())
	{
		return Some(today.add_days(-days));
	}
	let parts: Vec<&str> = text.split('-').collect();
	let n = |s: &str| s.parse::<u32>().ok();
	match parts.as_slice() {
		[y, m, d] if y.len() == 4 => {
			Date::new(n(y)?, n(m)? as u8, n(d)? as u8).ok()
		},
		[m, d] if m.len() <= 2 => {
			Date::new(today.year(), n(m)? as u8, n(d)? as u8).ok()
		},
		_ => None,
	}
}

/// Sorts the words of a quick entry. A date is only recognized first, so
/// that descriptions like "Sunday brunch" survive when a date comes first.
pub fn parse(
	words: &[String],
	today: Date,
	is_currency: &dyn Fn(&str) -> bool,
) -> Result<Quick, Diagnostic> {
	let mut tokens: Vec<String> = words
		.iter()
		.flat_map(|w| w.split_whitespace())
		.map(str::to_string)
		.collect();
	let mut quick = Quick::default();

	if let Some(date) = tokens.first().and_then(|t| parse_day(t, today, false))
	{
		quick.date = Some(date);
		tokens.remove(0);
	}

	// Anything shaped like a date is one, wherever it is, and never
	// arithmetic: `rent 09-01` is rent on September 1, not 8
	let mut i = 0;
	while i < tokens.len() {
		if !is_date_shaped(&tokens[i]) {
			i += 1;
			continue;
		}
		let token = tokens.remove(i);
		match (parse_day(&token, today, false), quick.date) {
			(Some(date), None) => quick.date = Some(date),
			(Some(_), Some(_)) => {
				return Err(Diagnostic::error(format!(
					"`{token}` looks like a second date"
				))
				.help("give one date, or put a subtraction in parentheses"));
			},
			(None, _) => {
				return Err(Diagnostic::error(format!(
					"`{token}` is not a valid date"
				))
				.help("dates are written like 03-15 or 2024-03-15"));
			},
		}
	}

	// Account hints
	tokens.retain(|t| {
		if let Some(hint) = t.strip_prefix('@').filter(|h| !h.is_empty()) {
			quick.hints.push(hint.to_string());
			false
		} else if t.contains(':') && check_account_prefix(t).is_ok() {
			quick.hints.push(t.clone());
			false
		} else {
			true
		}
	});

	// The amount is the last thing that reads as one, with a currency
	// either side of it if it names one the ledger uses
	let is_amount = |t: &str| {
		t.chars().any(|c| c.is_ascii_digit())
			&& !t.chars().any(|c| c.is_alphabetic() && !c.is_uppercase())
			&& expr::parse(t).is_ok()
	};
	if let Some(i) = tokens.iter().rposition(|t| is_amount(t)) {
		let mut typed = expr::parse(&tokens[i]).expect("checked above");
		tokens.remove(i);
		if typed.currency.is_none() {
			if tokens.get(i).is_some_and(|t| is_currency(t)) {
				typed.currency = Some(tokens.remove(i));
			} else if i > 0 && tokens.get(i - 1).is_some_and(|t| is_currency(t))
			{
				typed.currency = Some(tokens.remove(i - 1));
			}
		}
		quick.amount = Some(typed);
	}

	quick.description = tokens.join(" ");
	Ok(quick)
}

/// `03-15` or `2024-03-15`
fn is_date_shaped(token: &str) -> bool {
	let parts: Vec<&str> = token.split('-').collect();
	let digits =
		|p: &str| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit());
	match parts.as_slice() {
		[m, d] => digits(m) && digits(d) && m.len() <= 2 && d.len() <= 2,
		[y, m, d] => digits(y) && digits(m) && digits(d) && y.len() == 4,
		_ => false,
	}
}

/// How confidently a description matched a past one
fn strong(score: i64) -> bool {
	score >= SUBSTRING
}

/// Turns sorted words into a draft entry, using history to fill the gaps.
/// Fails, explaining what is missing, if there is not enough to go on.
pub fn resolve(
	quick: &Quick,
	history: &History,
	today: Date,
) -> Result<Draft, Diagnostic> {
	let date = quick.date.unwrap_or(today);
	let mut notes = vec![];

	// A past entry this is like, found by its description or its accounts
	let payee = find_payee(&quick.description, history, today);

	let hints = resolve_hints(&quick.hints, history, today)?;
	let mut draft = match payee {
		Some(payee) => {
			if !payee.is_simple() {
				return Err(Diagnostic::error(format!(
					"The last “{}” entry has prices, lots or several currencies, which quick add can't adapt",
					payee.name
				))
				.help(
					"run `ledr add` with no words to be asked for each line",
				));
			}
			notes.push(format!(
				"like your last “{}”, on {}",
				payee.name, payee.template_date
			));
			let mut draft = from_template(payee, quick, history, &mut notes)?;
			apply_hints(&mut draft, &hints);
			draft
		},
		None => from_scratch(quick, &hints, history, today, &mut notes)?,
	};

	draft.date = date;
	draft.notes = notes;
	Ok(draft)
}

fn find_payee<'a>(
	query: &str,
	history: &'a History,
	today: Date,
) -> Option<&'a Payee> {
	if query.trim().is_empty() {
		return None;
	}
	// Exactly a description used before
	if let Some(payee) = history
		.payees
		.values()
		.find(|p| p.name.eq_ignore_ascii_case(query.trim()))
	{
		return Some(payee);
	}

	// By name, e.g. "tim" for Tim Hortons, the best match winning and then
	// the most used
	let by_name = history
		.payees
		.values()
		.filter_map(|p| {
			let score = fuzzy::score(query, &p.name)?;
			strong(score).then(|| (p, (score, p.usage.weight(today))))
		})
		.max_by_key(|(_, rank)| *rank);
	if let Some((payee, _)) = by_name {
		return Some(payee);
	}
	// By what it was spent on, e.g. "coffee" for Expenses:Food:Coffee
	history
		.payees
		.values()
		.filter_map(|p| {
			let best = p
				.template
				.iter()
				.filter_map(|posting| fuzzy::score(query, &posting.account))
				.max()?;
			(best >= SEGMENT).then(|| (p, best * 10 + p.usage.weight(today)))
		})
		.max_by_key(|(_, s)| *s)
		.map(|(p, _)| p)
}

fn from_template(
	payee: &Payee,
	quick: &Quick,
	history: &History,
	notes: &mut Vec<String>,
) -> Result<Draft, Diagnostic> {
	let mut postings: Vec<DraftPosting> = payee
		.template
		.iter()
		.map(|p| DraftPosting {
			account: p.account.clone(),
			amount: p.amount.as_ref().map(|a| (a.value, a.currency.clone())),
		})
		.collect();

	if let Some(typed) = &quick.amount {
		let first = postings
			.iter()
			.position(|p| p.amount.is_some())
			.unwrap_or(0);
		let old = postings[first].amount.clone();
		let currency = typed
			.currency
			.clone()
			.or_else(|| old.as_ref().map(|(_, c)| c.clone()))
			.or_else(|| {
				history
					.currency_for(&postings[first].account)
					.map(String::from)
			})
			.ok_or_else(no_currency)?;

		// The amount is taken in the direction the template's went
		let direction = match &old {
			Some((v, _)) if v.is_negative() => -typed.value,
			_ => typed.value,
		};
		if postings.len() <= 2 {
			postings[first].amount = Some((direction, currency));
			for (i, p) in postings.iter_mut().enumerate() {
				if i != first {
					p.amount = None;
				}
			}
		} else {
			// Scale a split entry, like a paycheck, in proportion
			match old {
				Some((old_value, old_currency))
					if !old_value.is_zero() && old_currency == currency =>
				{
					let factor = typed.value / old_value.abs();
					for p in postings.iter_mut() {
						if let Some((v, c)) = &p.amount
							&& *c == currency
						{
							p.amount = Some((*v * factor, c.clone()));
						}
					}
					notes.push(
						"every line scaled in proportion to last time".into(),
					);
				},
				_ => postings[first].amount = Some((direction, currency)),
			}
		}
	} else {
		notes.push("same amounts as last time".into());
	}

	Ok(Draft {
		date: Date::min(),
		description: payee.name.clone(),
		postings,
		notes: vec![],
	})
}

fn no_currency() -> Diagnostic {
	Diagnostic::error("No currency for the amount")
		.help("write one after it, like `4.50 USD`")
}

/// Whether an account is where money is kept or owed (so it pays for
/// things), rather than what money was earned or spent on
fn is_funding(account: &str) -> bool {
	matches!(account.split(':').next(), Some("Assets" | "Liabilities"))
}

/// Swaps accounts in a reused entry for the hinted ones, each replacing a
/// line with the same role: `@checking` replaces whatever paid last time,
/// `@restaurants` whatever the money was for.
fn apply_hints(draft: &mut Draft, hints: &[String]) {
	let mut replaced = vec![false; draft.postings.len()];
	for hint in hints {
		if draft.postings.iter().any(|p| p.account == *hint) {
			continue;
		}
		let free = |i: &usize| !replaced[*i];
		let slot = (0..draft.postings.len())
			.filter(free)
			.find(|&i| {
				is_funding(&draft.postings[i].account) == is_funding(hint)
			})
			.or_else(|| (0..draft.postings.len()).find(free));
		if let Some(i) = slot {
			draft.postings[i].account = hint.clone();
			replaced[i] = true;
		}
	}
}

fn from_scratch(
	quick: &Quick,
	hints: &[String],
	history: &History,
	today: Date,
	notes: &mut Vec<String>,
) -> Result<Draft, Diagnostic> {
	let description = quick.description.trim();
	if description.is_empty() {
		return Err(Diagnostic::error("What was this for?")
			.help("describe it, like `ledr add groceries 54.20 @visa`"));
	}
	let example = |amount: &str| {
		format!("ledr add \"{description}\" {amount} @restaurants @checking")
	};
	let Some(typed) = &quick.amount else {
		return Err(Diagnostic::error(format!(
			"Nothing in your ledger looks like “{description}”, so I need an amount"
		))
		.help(format!("add one, like `{}`", example("12.50"))));
	};
	let amount_text = typed.value.to_string();

	// What the money was for, and what paid for it
	let (mut purposes, mut payers): (Vec<&String>, Vec<&String>) =
		hints.iter().partition(|h| !is_funding(h));
	if purposes.is_empty() && payers.len() >= 2 {
		// Two places money is kept: a transfer from the second to the first
		purposes.push(payers.remove(0));
	}

	let purpose = match purposes.first() {
		Some(p) => p.to_string(),
		None => {
			// The description might name the account, like "groceries"
			let found = history
				.find_accounts(description, today)
				.into_iter()
				.find(|(account, score)| {
					!is_funding(account)
						&& (fuzzy::score(description, account)
							.is_some_and(|s| s >= INITIALS)
							|| *score >= SEGMENT)
				});
			match found {
				Some((account, _)) => {
					notes.push(format!("{account}, from the description"));
					account.to_string()
				},
				None => {
					return Err(Diagnostic::error(format!(
						"What was “{description}” for?"
					))
					.help(format!(
						"name the account it was for, like `{}`",
						example(&amount_text)
					)));
				},
			}
		},
	};

	let payer = match payers.first().or(purposes.get(1)) {
		Some(p) => p.to_string(),
		None => {
			let partner = history
				.usual_partner_where(&purpose, is_funding)
				.map(String::from)
				.ok_or_else(|| {
					Diagnostic::error(format!(
						"Which account paid for “{description}”?"
					))
					.help("add a second account, like `@checking`")
				})?;
			notes.push(format!(
				"{partner}, since it usually goes with {purpose}"
			));
			partner
		},
	};

	let currency = typed
		.currency
		.clone()
		.or_else(|| history.currency_for(&purpose).map(String::from))
		.ok_or_else(no_currency)?;

	// Income is credited, so money earned is written as negative
	let value = if purpose.split(':').next() == Some("Income") {
		-typed.value
	} else {
		typed.value
	};

	Ok(Draft {
		date: today,
		description: capitalize(description),
		postings: vec![
			DraftPosting {
				account: purpose,
				amount: Some((value, currency)),
			},
			DraftPosting {
				account: payer,
				amount: None,
			},
		],
		notes: vec![],
	})
}

/// Matches each hint to a known account, or accepts it as a new one if it
/// is written out in full
fn resolve_hints(
	hints: &[String],
	history: &History,
	today: Date,
) -> Result<Vec<String>, Diagnostic> {
	hints
		.iter()
		.map(|hint| {
			if history.is_known_account(hint) {
				return Ok(hint.clone());
			}
			if hint.contains(':') && check_account_prefix(hint).is_ok() {
				return Ok(hint.clone());
			}
			history
				.find_accounts(hint, today)
				.into_iter()
				.find(|(account, _)| fuzzy::confident(hint, account))
				.map(|(a, _)| a.to_string())
				.ok_or_else(|| {
					Diagnostic::error(format!("No account matches `@{hint}`"))
						.help(
							"write the account in full to create it, like `@Expenses:Hobbies`",
						)
				})
		})
		.collect()
}

fn capitalize(s: &str) -> String {
	let mut chars = s.chars();
	match chars.next() {
		Some(first) => first.to_uppercase().chain(chars).collect(),
		None => String::new(),
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::input::history::History;
	use crate::parsing::loader::load;
	use crate::syntax::source::SourceMap;
	use crate::util::quant::Quant;

	const LEDGER: &str = "\
2024-01-02 Tim Hortons
    Expenses:Food:Coffee   4.50 CAD
    Liabilities:Visa

2024-01-05 Trader Joe's
    Expenses:Food:Groceries   54.20 USD
    Liabilities:Visa

2024-01-15 Paycheck
    Assets:Checking   2,000.00 USD
    Expenses:Taxes      500.00 USD
    Income:Salary    -2,500.00 USD
";

	fn history() -> History {
		let dir = tempfile::tempdir().unwrap();
		let path = dir.path().join("h.ledr");
		std::fs::write(&path, LEDGER).unwrap();
		let mut sources = SourceMap::new();
		History::build(
			&load(path.to_str().unwrap(), &mut sources, None).unwrap(),
		)
	}

	fn today() -> Date {
		Date::from_str("2024-03-13").unwrap() // a Wednesday
	}

	fn quick(words: &str) -> Result<Draft, Diagnostic> {
		let h = history();
		let words: Vec<String> = words.split(' ').map(String::from).collect();
		let q = parse(&words, today(), &|c| h.is_known_currency(c))?;
		resolve(&q, &h, today())
	}

	#[test]
	fn test_days() {
		let t = today();
		assert_eq!(
			parse_day("yesterday", t, false).unwrap().to_string(),
			"2024-03-12"
		);
		assert_eq!(
			parse_day("mon", t, false).unwrap().to_string(),
			"2024-03-11"
		);
		assert_eq!(
			parse_day("wednesday", t, false).unwrap().to_string(),
			"2024-03-13"
		);
		assert_eq!(
			parse_day("thu", t, false).unwrap().to_string(),
			"2024-03-07"
		);
		assert_eq!(
			parse_day("03-01", t, false).unwrap().to_string(),
			"2024-03-01"
		);
		assert_eq!(parse_day("-2", t, true).unwrap().to_string(), "2024-03-11");
		assert_eq!(parse_day("-2", t, false), None);
		assert_eq!(parse_day("pizza", t, false), None);
		assert_eq!(
			parse_day("su", t, false),
			None,
			"too short to be a weekday"
		);
	}

	#[test]
	fn test_parse_words() {
		let h = history();
		let words = ["fri", "Pizza", "night", "23.50", "CAD", "@visa"]
			.map(String::from);
		let q = parse(&words, today(), &|c| h.is_known_currency(c)).unwrap();
		assert_eq!(q.date.unwrap().to_string(), "2024-03-08");
		assert_eq!(q.description, "Pizza night");
		assert_eq!(q.hints, vec!["visa"]);
		let amount = q.amount.unwrap();
		assert_eq!(amount.currency.as_deref(), Some("CAD"));
		assert_eq!(amount.value, Quant::from_frac(47, 2));
	}

	#[test]
	fn test_reuses_the_last_entry_by_name() {
		let d = quick("tim 5.25").unwrap();
		assert_eq!(d.description, "Tim Hortons");
		assert_eq!(d.postings[0].account, "Expenses:Food:Coffee");
		assert_eq!(
			d.postings[0].amount,
			Some((Quant::from_frac(21, 4), "CAD".into()))
		);
		assert_eq!(d.postings[1].amount, None);
		assert_eq!(d.date, today());
	}

	#[test]
	fn test_finds_entries_by_what_they_were_for() {
		let d = quick("coffee 3").unwrap();
		assert_eq!(d.description, "Tim Hortons");
	}

	#[test]
	fn test_hints_replace_accounts_with_the_same_role() {
		// Paid from checking this time, instead of the Visa
		let d = quick("tim 5 @checking").unwrap();
		assert_eq!(d.postings[0].account, "Expenses:Food:Coffee");
		assert_eq!(d.postings[1].account, "Assets:Checking");
		// Or spent on groceries, still with the Visa
		let d = quick("tim 5 @groceries").unwrap();
		assert_eq!(d.postings[0].account, "Expenses:Food:Groceries");
		assert_eq!(d.postings[1].account, "Liabilities:Visa");
	}

	#[test]
	fn test_a_payer_alone_is_not_enough() {
		let e = quick("pizza night 23.50 @visa").unwrap_err();
		assert!(
			e.message.contains("What was “pizza night” for?"),
			"{}",
			e.message
		);
		let d = quick("pizza night 23.50 @groceries @visa").unwrap();
		assert_eq!(d.postings[0].account, "Expenses:Food:Groceries");
		assert_eq!(d.postings[1].account, "Liabilities:Visa");
	}

	#[test]
	fn test_transfers_between_accounts() {
		let d = quick("move money 100 @visa @checking").unwrap();
		assert_eq!(d.postings[0].account, "Liabilities:Visa");
		assert_eq!(
			d.postings[0].amount.as_ref().unwrap().0,
			Quant::from_i128(100)
		);
		assert_eq!(d.postings[1].account, "Assets:Checking");
	}

	#[test]
	fn test_dates_anywhere_are_never_amounts() {
		let d = quick("tim 09-01 3").unwrap();
		assert_eq!(d.date.to_string(), "2024-09-01");
		assert_eq!(
			d.postings[0].amount.as_ref().unwrap().0,
			Quant::from_i128(3)
		);
		let h = history();
		let words: Vec<String> =
			["tim", "02-30", "3"].map(String::from).to_vec();
		assert!(parse(&words, today(), &|c| h.is_known_currency(c)).is_err());
	}

	#[test]
	fn test_income_is_credited() {
		let d = quick("bonus 1000 @salary @checking").unwrap();
		assert_eq!(d.postings[0].account, "Income:Salary");
		assert_eq!(
			d.postings[0].amount.as_ref().unwrap().0,
			Quant::from_i128(-1000)
		);
	}

	#[test]
	fn test_hints_must_match_confidently() {
		assert!(quick("tim 5 @gas").unwrap_err().message.contains("@gas"));
		let d = quick("tim 5 @chk").unwrap();
		assert_eq!(d.postings[1].account, "Assets:Checking");
		// Naming an account the entry already uses changes nothing
		let d = quick("tim 5 @visa").unwrap();
		assert_eq!(d.postings[0].account, "Expenses:Food:Coffee");
		assert_eq!(d.postings[1].account, "Liabilities:Visa");
	}

	#[test]
	fn test_scales_split_entries() {
		let d = quick("paycheck 3000").unwrap();
		assert_eq!(
			d.postings[0].amount.as_ref().unwrap().0,
			Quant::from_i128(3000)
		);
		assert_eq!(
			d.postings[1].amount.as_ref().unwrap().0,
			Quant::from_i128(750)
		);
		assert_eq!(
			d.postings[2].amount.as_ref().unwrap().0,
			Quant::from_i128(-3750)
		);
	}

	#[test]
	fn test_new_descriptions_need_accounts() {
		let d = quick("groceries 12 ").unwrap();
		assert_eq!(d.description, "Trader Joe's", "matched by account");
		let d = quick("new bike 300 @Expenses:Hobbies @visa").unwrap();
		assert_eq!(d.description, "New bike");
		assert_eq!(d.postings[0].account, "Expenses:Hobbies");
		assert_eq!(d.postings[1].account, "Liabilities:Visa");
		let d = quick("new bike 300 @Expenses:Hobbies").unwrap_err();
		assert!(d.message.contains("Which account paid"));
		assert!(
			quick("mystery thing")
				.unwrap_err()
				.message
				.contains("need an amount")
		);
	}
}
