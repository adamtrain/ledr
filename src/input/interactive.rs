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

//! Asking for a new entry one question at a time, with suggestions.

use crate::gl::ledger::check_account_prefix;
use crate::input::compose::{Draft, DraftPosting, amount_text};
use crate::input::expr;
use crate::input::fuzzy::{self, Score};
use crate::input::history::History;
use crate::input::quick::{self, Quick, parse_day};
use crate::ui::style::{Rgb, palette};
use crate::util::date::Date;
use crate::util::quant::Quant;
use anyhow::Error;
use inquire::autocompletion::Replacement;
use inquire::ui::{Attributes, Color, RenderConfig, StyleSheet, Styled};
use inquire::validator::Validation;
use inquire::{Autocomplete, Confirm, CustomUserError, InquireError, Text};

/// How many suggestions to show under a prompt
const SUGGESTIONS: usize = 7;

fn color(c: Rgb) -> Color {
	Color::Rgb {
		r: c.0,
		g: c.1,
		b: c.2,
	}
}

/// Prompts in ledr's colors
pub fn style_prompts() {
	let faint = StyleSheet::new().with_fg(color(palette::FAINT));
	let config = RenderConfig::default_colored()
		.with_prompt_prefix(Styled::new("?").with_fg(color(palette::ACCENT)))
		.with_answered_prompt_prefix(
			Styled::new("✓").with_fg(color(palette::GREEN)),
		)
		.with_highlighted_option_prefix(
			Styled::new("❯").with_fg(color(palette::ACCENT)),
		)
		.with_selected_option(Some(
			StyleSheet::new()
				.with_fg(color(palette::ACCENT))
				.with_attr(Attributes::BOLD),
		))
		.with_help_message(faint)
		.with_default_value(faint)
		.with_answer(
			StyleSheet::new()
				.with_fg(color(palette::ACCENT))
				.with_attr(Attributes::BOLD),
		)
		.with_canceled_prompt_indicator(
			Styled::new("cancelled").with_fg(color(palette::FAINT)),
		);
	inquire::set_global_render_config(config);
}

/// Suggests from a fixed list, best fuzzy match first, then most used
#[derive(Clone)]
struct Suggest {
	options: Vec<(String, Score)>,
}

impl Autocomplete for Suggest {
	fn get_suggestions(
		&mut self,
		input: &str,
	) -> Result<Vec<String>, CustomUserError> {
		let ranked = fuzzy::rank(
			input,
			self.options.iter(),
			|(name, _)| name,
			|(_, w)| *w,
		);
		Ok(ranked
			.into_iter()
			.filter(|((name, _), _)| name != input)
			.take(SUGGESTIONS)
			.map(|((name, _), _)| name.clone())
			.collect())
	}

	fn get_completion(
		&mut self,
		input: &str,
		highlighted: Option<String>,
	) -> Result<Replacement, CustomUserError> {
		Ok(highlighted
			.or_else(|| self.get_suggestions(input).ok()?.into_iter().next()))
	}
}

fn accounts(history: &History, today: Date) -> Suggest {
	Suggest {
		options: history
			.all_accounts()
			.into_iter()
			.map(|a| {
				let weight =
					history.accounts.get(a).map_or(0, |u| u.weight(today));
				(a.to_string(), weight)
			})
			.collect(),
	}
}

fn payees(history: &History, today: Date) -> Suggest {
	Suggest {
		options: history
			.payees
			.values()
			.map(|p| (p.name.clone(), p.usage.weight(today)))
			.collect(),
	}
}

/// Whether the person gave up on the entry, as opposed to an error
pub fn is_cancel(error: &Error) -> bool {
	matches!(
		error.downcast_ref::<InquireError>(),
		Some(
			InquireError::OperationCanceled
				| InquireError::OperationInterrupted
		)
	)
}

/// Asks for an entry. `start` holds anything already known, e.g. from a
/// quick description that was not enough on its own.
pub fn ask(
	history: &History,
	today: Date,
	start: &Quick,
) -> Result<Draft, Error> {
	let default_date = start.date.unwrap_or(today).to_string();
	let format_date = |s: &str| {
		parse_day(s, today, true).map_or(s.to_string(), |d| d.to_string())
	};
	let date_text = Text::new("Date")
		.with_default(&default_date)
		.with_help_message(
			"today · yesterday · a weekday like fri · 03-15 · -2 for two days ago",
		)
		.with_validator(move |s: &str| {
			Ok(match parse_day(s, today, true) {
				Some(_) => Validation::Valid,
				None => {
					Validation::Invalid("That isn't a date I understand".into())
				},
			})
		})
		.with_formatter(&format_date)
		.prompt()?;
	let date = parse_day(&date_text, today, true).unwrap_or(today);

	let description = Text::new("Description")
		.with_initial_value(&start.description)
		.with_autocomplete(payees(history, today))
		.with_help_message(
			"type to search past entries · ↑↓ to choose · tab to complete",
		)
		.with_validator(|s: &str| {
			Ok(if s.trim().is_empty() {
				Validation::Invalid(
					"Describe the entry, e.g. who was paid".into(),
				)
			} else if s.contains('#') {
				Validation::Invalid(
					"`#` starts a comment, so it can't be in a description"
						.into(),
				)
			} else {
				Validation::Valid
			})
		})
		.prompt()?;
	let description = description.trim().to_string();

	// A description used before can reuse that entry's accounts
	if let Some(payee) =
		history.payees.get(&description).filter(|p| p.is_simple())
	{
		let summary: Vec<String> = payee
			.template
			.iter()
			.map(|p| match &p.amount {
				Some(a) => {
					format!("{} {} {}", p.account, a.value_text, a.currency)
				},
				None => p.account.clone(),
			})
			.collect();
		let same = Confirm::new("Same accounts as last time?")
			.with_default(true)
			.with_help_message(&format!(
				"{} on {}",
				summary.join(" · "),
				payee.template_date
			))
			.prompt()?;
		if same {
			let last = payee.template.iter().find_map(|p| p.amount.as_ref());
			let default =
				last.map(|a| format!("{} {}", a.value_text, a.currency));
			let mut prompt = Text::new("Amount")
				.with_help_message(
					"arithmetic works, like 12.50+3.20 · add a currency to change it",
				)
				.with_validator(|s: &str| {
					Ok(match expr::parse(s) {
						Ok(_) => Validation::Valid,
						Err(e) => Validation::Invalid(format!("{e}").into()),
					})
				});
			if let Some(default) = &default {
				prompt = prompt.with_default(default);
			}
			let amount = expr::parse(&prompt.prompt()?)?;
			let quick = Quick {
				date: Some(date),
				amount: Some(amount),
				hints: vec![],
				description: description.clone(),
			};
			let mut draft = quick::resolve(&quick, history, today)?;
			draft.description = description;
			return Ok(draft);
		}
	}

	let template: Vec<String> = history
		.payees
		.get(&description)
		.map(|p| p.template.iter().map(|t| t.account.clone()).collect())
		.unwrap_or_default();

	let mut draft = Draft {
		date,
		description,
		postings: vec![],
		notes: vec![],
	};
	let hint_accounts = start.hints.clone();
	loop {
		let n = draft.postings.len() + 1;
		let can_finish = draft.is_balanced();
		let remaining = draft.imbalance();
		let help = if can_finish {
			"press enter on an empty line to finish".to_string()
		} else if remaining.is_empty() {
			"type to search accounts · a new one is fine too".to_string()
		} else {
			format!("still to balance: {}", show(&remaining, true))
		};

		let default = template
			.get(n - 1)
			.or(hint_accounts.get(n - 1))
			.filter(|_| !can_finish)
			.cloned();
		let label = format!("Account {n}");
		let mut prompt = Text::new(&label)
			.with_autocomplete(accounts(history, today))
			.with_help_message(&help)
			.with_validator(move |s: &str| {
				let s = s.trim();
				Ok(if s.is_empty() {
					if can_finish {
						Validation::Valid
					} else {
						Validation::Invalid(
							"The entry doesn't balance yet".into(),
						)
					}
				} else {
					if s.contains(char::is_whitespace) || s.contains('#') {
						return Ok(Validation::Invalid(
							"Account names can't contain spaces or `#`".into(),
						));
					}
					match check_account_prefix(s) {
						Ok(()) => Validation::Valid,
						Err(e) => Validation::Invalid(
							e.help.unwrap_or(e.message).into(),
						),
					}
				})
			});
		if let Some(default) = &default {
			prompt = prompt.with_default(default);
		}
		let account = prompt.prompt()?.trim().to_string();
		if account.is_empty() {
			break;
		}

		let blank_allowed = !draft.has_blank();
		let suggestion = match remaining.len() {
			1 => remaining.iter().next().map(|(c, v)| (-*v, c.clone())),
			_ => None,
		};
		let amount_help = match (&suggestion, blank_allowed) {
			(Some((v, c)), true) => format!("leave blank to balance ({} {c}) · arithmetic works", show_one(*v)),
			(_, true) => "leave blank to balance the rest · arithmetic works, like 12.50+3.20".into(),
			_ => "arithmetic works, like 12.50+3.20".into(),
		};
		let typed = Text::new("Amount")
			.with_formatter(&|s: &str| {
				if s.trim().is_empty() {
					"(balances the entry)".to_string()
				} else {
					computed(s)
				}
			})
			.with_help_message(&amount_help)
			.with_validator(move |s: &str| {
				Ok(if s.trim().is_empty() {
					if blank_allowed {
						Validation::Valid
					} else {
						Validation::Invalid(
							"Only one line can be left blank".into(),
						)
					}
				} else {
					match expr::parse(s) {
						Ok(_) => Validation::Valid,
						Err(e) => Validation::Invalid(format!("{e}").into()),
					}
				})
			})
			.prompt()?;

		let amount = if typed.trim().is_empty() {
			None
		} else {
			let parsed = expr::parse(&typed)?;
			let currency = parsed
				.currency
				.or_else(|| history.currency_for(&account).map(String::from))
				.or_else(|| suggestion.as_ref().map(|(_, c)| c.clone()))
				.unwrap_or_else(|| "USD".into());
			Some((parsed.value, currency))
		};
		let blank = amount.is_none();
		draft.postings.push(DraftPosting { account, amount });

		// A blank line balances everything, so the entry is done
		if blank || (draft.postings.len() >= 2 && draft.imbalance().is_empty())
		{
			break;
		}
	}
	Ok(draft)
}

fn show_one(value: Quant) -> String {
	amount_text(value, "", &|_| 2)
}

fn show(
	amounts: &std::collections::BTreeMap<String, Quant>,
	negate: bool,
) -> String {
	amounts
		.iter()
		.map(|(c, v)| {
			let v = if negate { -*v } else { *v };
			format!("{} {c}", show_one(v))
		})
		.collect::<Vec<_>>()
		.join(", ")
}

/// Shows a typed amount as what it works out to, e.g. `4.25+1.10` as `5.35`
fn computed(input: &str) -> String {
	match expr::parse(input) {
		Ok(typed) if typed.computed => {
			let value = show_one(typed.value);
			match typed.currency {
				Some(c) => format!("{value} {c}"),
				None => value,
			}
		},
		_ => input.to_string(),
	}
}

/// Asks whether to go ahead
pub fn confirm(question: &str) -> Result<bool, Error> {
	Ok(Confirm::new(question).with_default(true).prompt()?)
}
