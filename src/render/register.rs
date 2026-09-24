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

//! The account summary: a balance history and register.

use crate::render::{account, category_color, dotted, plural};
use crate::reports::register::Register;
use crate::ui::style::{Style, palette};
use crate::ui::text::{Doc, Line};
use crate::ui::widgets::{
	Column, Decimals, Table, badge, bar, panel, rule, sparkline,
};
use crate::util::quant::Quant;

/// How many descriptions to list in the "Top descriptions" section
const TOP: usize = 10;

pub fn register(report: &Register, total_width: usize) -> Doc {
	let mut doc = Doc::new();
	let inner = total_width - 6;

	if report.rows.is_empty() {
		doc.extend(panel(
			vec![
				badge("ACCOUNT", palette::FAINT)
					.with(format!("  {}", report.pattern), Style::new().bold()),
				Line::new(),
				Line::faint(format!(
					"No entries touch an account matching “{}”.",
					report.pattern
				)),
			],
			palette::FAINT,
			None,
			total_width,
		));
		return doc;
	}

	let color = match report.accounts.as_slice() {
		[only] => category_color(only),
		_ => palette::ACCENT,
	};

	// Heading: the account, or how many matched
	let mut body = vec![];
	let mut title = badge("ACCOUNT", color).with("  ", Style::new());
	match report.accounts.as_slice() {
		[only] => title.append(account(only)),
		many => {
			title.push(
				format!("{} matching ", plural(many.len(), "account")),
				Style::new(),
			);
			title.push(format!("“{}”", report.pattern), Style::new().bold());
		},
	}
	body.push(title);
	body.push(Line::new());

	// Balance by currency, as of the last entry shown
	let numbers: Vec<String> =
		report.closing.iter().map(|(_, v)| v.to_string()).collect();
	let decimals = Decimals::fit(numbers.iter().map(String::as_str));
	for (i, (currency, value)) in report.closing.iter().enumerate() {
		body.push(
			Line::plain(if i == 0 {
				"Balance     "
			} else {
				"            "
			})
			.with(decimals.align(&value.to_string()), Style::new().bold())
			.with(format!(" {currency}"), Style::faint()),
		);
	}

	// Balance history, for the currency used most
	let main = main_currency(report);
	if let Some(currency) = &main {
		let opening = report
			.opening
			.iter()
			.find(|(c, _)| c == currency)
			.map(|(_, v)| v.to_f64());
		let history: Vec<f64> = opening
			.into_iter()
			.chain(
				report
					.rows
					.iter()
					.filter_map(|r| {
						r.balances.iter().find(|(c, _)| c == currency)
					})
					.map(|(_, v)| v.to_f64()),
			)
			.collect();
		if history.len() > 1 {
			let first = report
				.rows
				.first()
				.map(|r| r.date.to_string())
				.unwrap_or_default();
			let last = report
				.rows
				.last()
				.map(|r| r.date.to_string())
				.unwrap_or_default();
			let span = format!("  {first} → {last}");
			let cells = inner.saturating_sub(span.chars().count()).min(60);
			body.push(Line::new());
			body.push(
				Line::styled(sparkline(&history, cells), Style::color(color))
					.with(span, Style::faint()),
			);
		}
	}

	let meta = vec![
		plural(report.rows.len(), "entry"),
		if report.accounts.len() > 1 {
			plural(report.accounts.len(), "account")
		} else {
			String::new()
		},
	];
	doc.extend(panel(body, color, Some(dotted(&meta)), total_width));
	doc.blank();

	// The register itself
	doc.push(rule("Register", total_width));
	let mut table = Table::new(vec![
		Column::left("Date"),
		Column::left("Description").shrinking(),
		Column::right("Amount"),
		Column::right("Balance"),
		Column::left("Reference").shrinking(),
	]);
	let amounts: Vec<String> = report
		.rows
		.iter()
		.flat_map(|r| r.amounts.iter().map(|(_, v)| v.to_string()))
		.collect();
	let balances: Vec<String> = report
		.rows
		.iter()
		.flat_map(|r| r.balances.iter().map(|(_, v)| v.to_string()))
		.collect();
	let amount_col = Decimals::fit(amounts.iter().map(String::as_str));
	let balance_col = Decimals::fit(balances.iter().map(String::as_str));

	for row in &report.rows {
		for (i, ((currency, value), (_, balance))) in
			row.amounts.iter().zip(&row.balances).enumerate()
		{
			let first = i == 0;
			table.row(vec![
				if first {
					Line::faint(row.date.to_string())
				} else {
					Line::new()
				},
				if first {
					Line::plain(row.description.clone())
				} else {
					Line::faint("  ↳")
				},
				signed(&amount_col.align(&value.to_string()), value, currency),
				Line::plain(balance_col.align(&balance.to_string()))
					.with(format!(" {currency}"), Style::faint()),
				if first {
					Line::faint(row.reference.replace('\n', " "))
				} else {
					Line::new()
				},
			]);
		}
	}
	doc.extend(table.render(total_width));

	// Where the money went, or came from
	if report.by_description.len() > 1 {
		doc.blank();
		doc.push(rule("Top descriptions", total_width));
		doc.extend(top_descriptions(report, total_width));
	}

	doc
}

/// The currency that moves in the most entries of the register
fn main_currency(report: &Register) -> Option<String> {
	let mut counts: std::collections::BTreeMap<&str, usize> =
		Default::default();
	for row in &report.rows {
		for (currency, _) in &row.amounts {
			*counts.entry(currency.as_str()).or_default() += 1;
		}
	}
	counts
		.into_iter()
		.max_by_key(|(_, n)| *n)
		.map(|(c, _)| c.to_string())
}

fn signed(number: &str, value: &Quant, currency: &str) -> Line {
	let style = if value.is_negative() {
		Style::color(palette::RED)
	} else {
		Style::new()
	};
	Line::styled(number, style).with(format!(" {currency}"), Style::faint())
}

fn top_descriptions(report: &Register, total_width: usize) -> Vec<Line> {
	let Some(currency) = main_currency(report) else {
		return vec![];
	};
	let currency = &currency;

	// Ranked by size of movement in the main currency
	let mut ranked: Vec<(&str, Quant, usize)> = report
		.by_description
		.iter()
		.filter_map(|d| {
			let (_, value) = d.amounts.iter().find(|(c, _)| c == currency)?;
			Some((d.description.as_str(), *value, d.count))
		})
		.collect();
	ranked.sort_by(|a, b| b.1.abs().cmp(&a.1.abs()).then(a.0.cmp(b.0)));
	let more = ranked.len().saturating_sub(TOP);
	ranked.truncate(TOP);

	let largest = ranked.first().map_or(0.0, |r| r.1.abs().to_f64());
	let numbers: Vec<String> = ranked.iter().map(|r| r.1.to_string()).collect();
	let decimals = Decimals::fit(numbers.iter().map(String::as_str));

	let mut table = Table::new(vec![
		Column::left("").shrinking(),
		Column::right(""),
		Column::right(""),
		Column::left(""),
	]);
	for (desc, value, count) in &ranked {
		let color = if value.is_negative() {
			palette::RED
		} else {
			palette::GREEN
		};
		table.row(vec![
			Line::plain(*desc),
			signed(&decimals.align(&value.to_string()), value, currency),
			Line::faint(format!("{count}×")),
			bar(
				value.abs().to_f64() / largest.max(f64::MIN_POSITIVE),
				24,
				color,
			),
		]);
	}
	let mut lines = table.render(total_width);
	if more > 0 {
		lines.push(Line::faint(format!("… and {more} more")));
	}
	lines
}
