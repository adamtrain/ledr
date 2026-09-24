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

//! Asking questions one at a time, with suggestions: for a new entry, or
//! for a new ledger.

use crate::config;
use crate::gl::ledger::check_account_prefix;
use crate::input::compose::{Draft, DraftPosting, amount_text};
use crate::input::expr;
use crate::input::fuzzy::{self, Score};
use crate::input::history::History;
use crate::input::quick::{self, Quick, parse_day};
use crate::input::setup::{self, Opening};
use crate::ui::style::{Rgb, palette};
use crate::util::date::Date;
use crate::util::quant::Quant;
use anyhow::Error;
use inquire::autocompletion::Replacement;
use inquire::ui::{Attributes, Color, RenderConfig, StyleSheet, Styled};
use inquire::validator::Validation;
use inquire::{
	Autocomplete, Confirm, CustomUserError, InquireError, MultiSelect, Text,
};
use std::path::PathBuf;

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
		)
		.with_selected_checkbox(
			Styled::new("◉").with_fg(color(palette::ACCENT)),
		)
		.with_unselected_checkbox(
			Styled::new("○").with_fg(color(palette::FAINT)),
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

/// Asks a yes or no question, with a note under it
pub fn confirm_with(
	question: &str,
	default: bool,
	help: &str,
) -> Result<bool, Error> {
	Ok(Confirm::new(question)
		.with_default(default)
		.with_help_message(help)
		.prompt()?)
}

/// Suggests files and folders as a path is typed
#[derive(Clone)]
struct Paths;

impl Autocomplete for Paths {
	fn get_suggestions(
		&mut self,
		input: &str,
	) -> Result<Vec<String>, CustomUserError> {
		let mut found = path_completions(input);
		found.truncate(SUGGESTIONS);
		Ok(found)
	}

	fn get_completion(
		&mut self,
		input: &str,
		highlighted: Option<String>,
	) -> Result<Replacement, CustomUserError> {
		if highlighted.is_some() {
			return Ok(highlighted);
		}
		// As far as every possibility agrees, like a shell does
		let found = path_completions(input);
		let Some((first, rest)) = found.split_first() else {
			return Ok(None);
		};
		let common = rest.iter().fold(first.clone(), |common, other| {
			common
				.chars()
				.zip(other.chars())
				.take_while(|(a, b)| a == b)
				.map(|(a, _)| a)
				.collect()
		});
		Ok((common.chars().count() > input.chars().count()).then_some(common))
	}
}

/// Files and folders that could finish a typed path, folders ending in `/`,
/// hidden ones only when a name starting with `.` is typed
fn path_completions(input: &str) -> Vec<String> {
	if input == "~" {
		return vec!["~/".into()];
	}
	if input.is_empty() {
		return vec![];
	}
	let (folder, start) = match input.rfind('/') {
		Some(i) => input.split_at(i + 1),
		None => ("", input),
	};
	let dir = if folder.is_empty() {
		PathBuf::from(".")
	} else {
		config::expand_home(folder)
	};
	let Ok(entries) = std::fs::read_dir(&dir) else {
		return vec![];
	};
	let start_lower = start.to_lowercase();
	let mut found: Vec<String> = entries
		.filter_map(Result::ok)
		.filter_map(|entry| {
			let name = entry.file_name().into_string().ok()?;
			if (name.starts_with('.') && !start.starts_with('.'))
				|| !name.to_lowercase().starts_with(&start_lower)
			{
				return None;
			}
			let slash = if entry.path().is_dir() { "/" } else { "" };
			Some(format!("{folder}{name}{slash}"))
		})
		.filter(|path| path != input)
		.collect();
	found.sort_by_key(|path| path.to_lowercase());
	found
}

/// Asks where a new ledger should go, as typed
pub fn ask_path(default: &str) -> Result<String, Error> {
	Ok(Text::new("Where should your ledger go?")
		.with_default(default)
		.with_autocomplete(Paths)
		.with_help_message(
			"a file to create, or a folder to put main.ledr in · tab completes",
		)
		.with_validator(|s: &str| {
			let path = setup::resolve_path(s);
			Ok(match setup::check_new_path(&path) {
				Ok(()) => Validation::Valid,
				Err(e) => Validation::Invalid(e.into()),
			})
		})
		.with_formatter(&|s: &str| config::display(&setup::resolve_path(s)))
		.prompt()?)
}

/// Asks for the currency most things are in
pub fn ask_currency(default: &str, help: &str) -> Result<String, Error> {
	let options = setup::currency_codes()
		.enumerate()
		.map(|(i, code)| (code.to_string(), -(i as Score)))
		.collect();
	let answer = Text::new("Main currency")
		.with_default(default)
		.with_autocomplete(Suggest { options })
		.with_help_message(help)
		.with_validator(|s: &str| {
			Ok(match setup::check_currency(s) {
				Ok(_) => Validation::Valid,
				Err(e) => Validation::Invalid(e.into()),
			})
		})
		.with_formatter(&|s: &str| s.trim().to_uppercase())
		.prompt()?;
	setup::check_currency(&answer).map_err(Error::msg)
}

/// Asks when the books start
pub fn ask_start(today: Date) -> Result<Date, Error> {
	let format = |s: &str| {
		parse_day(s, today, true).map_or(s.to_string(), |d| d.to_string())
	};
	let answer = Text::new("Balances as of")
		.with_default(&today.to_string())
		.with_help_message(
			"usually today · or the day your records start, like 2026-01-01",
		)
		.with_validator(move |s: &str| {
			Ok(match parse_day(s, today, true) {
				Some(_) => Validation::Valid,
				None => {
					Validation::Invalid("That isn't a date I understand".into())
				},
			})
		})
		.with_formatter(&format)
		.prompt()?;
	Ok(parse_day(&answer, today, true).unwrap_or(today))
}

/// Asks for accounts under `category`, and what each held on `date`, until
/// an empty name. Money owed is asked for as a positive amount, and kept
/// negative, as it is posted.
pub fn ask_openings(
	category: &str,
	suggestions: &[&str],
	currency: &str,
	date: Date,
	openings: &mut Vec<Opening>,
) -> Result<(), Error> {
	let owed = category == "Liabilities";
	let example = suggestions.first().copied().unwrap_or("Checking");
	loop {
		let taken: Vec<String> =
			openings.iter().map(|o| o.account.clone()).collect();
		let options = suggestions
			.iter()
			.filter(|s| !taken.contains(&format!("{category}:{s}")))
			.enumerate()
			.map(|(i, s)| (s.to_string(), -(i as Score)))
			.collect();
		let any_yet = taken.iter().any(|a| a.starts_with(category));
		let help = if any_yet {
			"another, or enter on an empty line to move on".to_string()
		} else {
			format!(
				"a name like {example}, or Bank:{example} · ↑↓ and tab pick a \
				suggestion · enter on an empty line to skip"
			)
		};
		let name = Text::new("Account")
			.with_autocomplete(Suggest { options })
			.with_help_message(&help)
			.with_validator({
				let category = category.to_string();
				move |s: &str| {
					if s.trim().is_empty() {
						return Ok(Validation::Valid);
					}
					Ok(match setup::account_name(&category, s) {
						Ok(a) if taken.contains(&a) => Validation::Invalid(
							"That one's already here".into(),
						),
						Ok(_) => Validation::Valid,
						Err(e) => Validation::Invalid(e.into()),
					})
				}
			})
			.with_formatter(&|s: &str| {
				if s.trim().is_empty() {
					"that's all".into()
				} else {
					setup::account_name(category, s)
						.unwrap_or_else(|_| s.to_string())
				}
			})
			.prompt()?;
		if name.trim().is_empty() {
			return Ok(());
		}
		let account =
			setup::account_name(category, &name).map_err(Error::msg)?;

		let (question, help) = if owed {
			(
				"Owed",
				format!(
					"how much you owed on {date} · arithmetic works · blank if \
					nothing"
				),
			)
		} else {
			(
				"Balance",
				format!(
					"what it held on {date} · arithmetic works · add a currency \
					if it isn't {currency} · blank if nothing"
				),
			)
		};
		let shown = |s: &str| -> String {
			if s.trim().is_empty() {
				return "nothing".into();
			}
			match expr::parse(s) {
				Ok(typed) => {
					let currency = setup::currency_of(s, &typed, currency);
					format!(
						"{} {currency}",
						amount_text(typed.value, &currency, &setup::decimals)
					)
				},
				Err(_) => s.to_string(),
			}
		};
		let typed = Text::new(question)
			.with_help_message(&help)
			.with_validator(|s: &str| {
				Ok(if s.trim().is_empty() {
					Validation::Valid
				} else {
					match expr::parse(s) {
						Ok(_) => Validation::Valid,
						Err(e) => Validation::Invalid(format!("{e}").into()),
					}
				})
			})
			.with_formatter(&shown)
			.prompt()?;
		let balance = if typed.trim().is_empty() {
			None
		} else {
			let parsed = expr::parse(&typed)?;
			let currency = setup::currency_of(&typed, &parsed, currency);
			let value = if owed { -parsed.value } else { parsed.value };
			Some((value, currency))
		};
		openings.push(Opening { account, balance });
	}
}

/// Asks which of `presets` to start with, as accounts under `category`
pub fn ask_categories(
	question: &str,
	category: &str,
	presets: &[(&str, bool)],
) -> Result<Vec<String>, Error> {
	let options: Vec<&str> = presets.iter().map(|(name, _)| *name).collect();
	let chosen: Vec<usize> = presets
		.iter()
		.enumerate()
		.filter(|(_, (_, on))| *on)
		.map(|(i, _)| i)
		.collect();
	let picked = MultiSelect::new(question, options)
		.with_default(&chosen)
		.with_page_size(12)
		.with_help_message(
			"space to choose · → all · ← none · type to filter · more can be \
			added any time",
		)
		.with_formatter(&|picked| match picked.len() {
			0 => "none".into(),
			n => format!("{n} chosen"),
		})
		.prompt()?;
	Ok(picked
		.into_iter()
		.map(|name| format!("{category}:{name}"))
		.collect())
}
