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

//! Realized and unrealized gains, as a headline panel over a lot table.

use crate::render::{dotted, gain, gain_color, percent, plural};
use crate::reports::portfolio_reporter::{
	GainReport, PortfolioReporter, RealizedRow, UnrealizedRow, show_rate,
};
use crate::ui::style::{Style, palette};
use crate::ui::text::{Doc, Line};
use crate::ui::widgets::{Column, Table, badge, meter, panel, rule};
use crate::util::date::{Date, Duration};
use crate::util::quant::Quant;
use std::collections::BTreeSet;

/// A holding period of at least a year gets called out, since many tax
/// systems treat those differently
fn held(duration: &Duration) -> Line {
	let text = duration.compact();
	if duration.years() >= 1 {
		Line::styled(text, Style::color(palette::ACCENT))
	} else {
		Line::plain(text)
	}
}

fn headline<Row>(
	reporter: &PortfolioReporter,
	report: &GainReport<Row>,
	title: &str,
	period: String,
	counts: Vec<String>,
) -> (Vec<Line>, crate::ui::style::Rgb) {
	let known: Vec<(&String, Quant)> = report
		.totals
		.iter()
		.filter_map(|(c, t)| t.map(|t| (c, t)))
		.collect();
	let color = if known.is_empty() {
		palette::FAINT
	} else if known.iter().all(|(_, t)| !t.is_negative()) {
		palette::GREEN
	} else if known.iter().all(|(_, t)| t.is_negative()) {
		palette::RED
	} else {
		palette::ACCENT
	};

	let mut body = vec![
		badge(title, color).with(format!("  {period}"), Style::faint()),
		Line::new(),
	];
	for (currency, total) in &report.totals {
		let cost = report.costs.get(currency).copied().unwrap_or_default();
		match total {
			Some(t) => {
				let mut line = gain(&reporter.show(*t, currency), t, currency)
					.with("   ", Style::new());
				if !cost.is_zero() {
					let fraction = (*t / cost.abs()).to_f64();
					let sign = if t.is_negative() { "" } else { "+" };
					line.push(
						format!("{sign}{} on cost", percent(fraction)),
						Style::faint(),
					);
				}
				body.push(line);
			},
			None => body.push(
				Line::styled("? ", Style::faint())
					.with("unknown", Style::new().bold())
					.with(
						format!(
							" {currency}  some gains below could not be valued"
						),
						Style::faint(),
					),
			),
		}
	}
	body.push(Line::new());
	body.push(dotted(&counts));
	(body, color)
}

pub fn realized(
	reporter: &PortfolioReporter,
	report: &GainReport<RealizedRow>,
	period: String,
	total_width: usize,
) -> Doc {
	let mut doc = Doc::new();
	if report.rows.is_empty() {
		doc.extend(panel(
			vec![
				badge("REALIZED", palette::FAINT)
					.with(format!("  {period}"), Style::faint()),
				Line::new(),
				Line::faint("No lots were sold in this period."),
			],
			palette::FAINT,
			None,
			total_width,
		));
		return doc;
	}

	let lots: BTreeSet<&str> =
		report.rows.iter().map(|r| r.lot_id.as_str()).collect();
	let assets: BTreeSet<&str> =
		report.rows.iter().map(|r| r.asset.as_str()).collect();
	let (body, color) = headline(
		reporter,
		report,
		"REALIZED",
		period,
		vec![
			plural(report.rows.len(), "sale"),
			plural(lots.len(), "lot"),
			plural(assets.len(), "asset"),
		],
	);
	doc.extend(panel(body, color, None, total_width));
	doc.blank();

	doc.push(rule("Sales", total_width));
	let mut table = Table::new(vec![
		Column::left("Lot").shrinking(),
		Column::left("Asset"),
		Column::right("Qty"),
		Column::left("Opened → Closed"),
		Column::right("Held"),
		Column::right("Cost"),
		Column::right("Proceeds"),
		Column::right("Gain"),
	]);

	let mut notes: Vec<Line> = vec![];
	let mut unknown = vec![];
	for row in &report.rows {
		let proceeds = match &row.proceeds {
			Some((value, currency)) => {
				Line::plain(reporter.show(*value, currency))
					.with(format!(" {currency}"), Style::faint())
			},
			None => Line::faint("unknown"),
		};
		let gain_cell = match &row.gain {
			Some(g) => {
				let mut cell = gain(
					&reporter.show(g.total, &row.cost_currency),
					&g.total,
					&row.cost_currency,
				);
				if let (Some(rate), Some((_, from))) = (&g.rate, &row.proceeds)
				{
					notes.push(
						Line::faint(format!("{} ", notes.len() + 1)).with(
							format!(
								"Lot {} proceeds converted from {from} at {} {}/{from} on {}",
								row.lot_id,
								show_rate(*rate),
								row.cost_currency,
								row.closed
							),
							Style::faint(),
						),
					);
					cell.push(superscript(notes.len()), Style::faint());
				}
				cell
			},
			None => {
				unknown.push(row.lot_id.clone());
				Line::faint("unknown")
			},
		};
		table.row(vec![
			Line::plain(row.lot_id.clone()),
			Line::plain(row.asset.clone()),
			Line::plain(reporter.show(row.quantity, &row.asset)),
			Line::faint(row.opened.to_string())
				.with(" → ", Style::faint())
				.with(row.closed.to_string(), Style::new()),
			held(&row.held),
			Line::plain(reporter.show(row.cost, &row.cost_currency))
				.with(format!(" {}", row.cost_currency), Style::faint()),
			proceeds,
			gain_cell,
		]);
	}
	doc.extend(table.render(total_width));

	if !notes.is_empty() || !unknown.is_empty() {
		doc.blank();
		doc.extend(notes);
		if !unknown.is_empty() {
			doc.push(
				Line::styled("○ ", Style::faint())
					.with(
						format!(
							"{} ",
							if unknown.len() == 1 { "Lot" } else { "Lots" }
						),
						Style::new().bold(),
					)
					.with(unknown.join(", "), Style::new().bold())
					.with(
						" had no proceeds ledr could value. Record each sale in its own \
						entry with its proceeds, or declare an exchange rate for them.",
						Style::new(),
					),
			);
		}
	}
	doc
}

pub fn unrealized(
	reporter: &PortfolioReporter,
	report: &GainReport<UnrealizedRow>,
	as_of: &Date,
	total_width: usize,
) -> Doc {
	let mut doc = Doc::new();
	if report.rows.is_empty() {
		doc.extend(panel(
			vec![
				badge("UNREALIZED", palette::FAINT)
					.with(format!("  as of {as_of}"), Style::faint()),
				Line::new(),
				Line::faint("There are no open lots."),
			],
			palette::FAINT,
			None,
			total_width,
		));
		return doc;
	}

	let assets: BTreeSet<&str> =
		report.rows.iter().map(|r| r.asset.as_str()).collect();
	let (body, color) = headline(
		reporter,
		report,
		"UNREALIZED",
		format!("as of {as_of}"),
		vec![
			plural(report.rows.len(), "open lot"),
			plural(assets.len(), "asset"),
		],
	);
	doc.extend(panel(body, color, None, total_width));
	doc.blank();

	doc.push(rule("Open lots", total_width));
	let mut table = Table::new(vec![
		Column::left("Lot").shrinking(),
		Column::left("Asset"),
		Column::right("Qty"),
		Column::left("Opened"),
		Column::right("Held"),
		Column::right("Cost"),
		Column::right("Price"),
		Column::right("Gain"),
		Column::left(""),
	]);

	// How much each lot has moved, relative to its cost
	let change = |row: &UnrealizedRow| -> Option<f64> {
		let g = row.gain.as_ref()?;
		Some(if row.cost.is_zero() {
			0.0
		} else {
			(g.unit / row.cost.abs()).to_f64()
		})
	};
	let largest = report
		.rows
		.iter()
		.filter_map(change)
		.map(f64::abs)
		.fold(0.0, f64::max);

	let mut unpriced = vec![];
	for row in &report.rows {
		let (price, gain_cell, chart) = match &row.gain {
			Some(g) => {
				let change = change(row).unwrap_or(0.0);
				let sign = if change > 0.0 { "+" } else { "" };
				(
					Line::plain(reporter.show(g.price, &row.cost_currency))
						.with(
							format!(" {}", row.cost_currency),
							Style::faint(),
						),
					gain(
						&reporter.show(g.total, &row.cost_currency),
						&g.total,
						&row.cost_currency,
					),
					meter(
						change.abs() / largest.max(f64::MIN_POSITIVE),
						8,
						gain_color(&g.total),
					)
					.with(
						format!(" {sign}{}", percent(change)),
						Style::color(gain_color(&g.total)),
					),
				)
			},
			None => {
				unpriced.push(row);
				(Line::faint("no price"), Line::faint("unknown"), Line::new())
			},
		};
		table.row(vec![
			Line::plain(row.lot_id.clone()),
			Line::plain(row.asset.clone()),
			Line::plain(reporter.show(row.quantity, &row.asset)),
			Line::faint(row.opened.to_string()),
			held(&row.held),
			Line::plain(reporter.show(row.cost, &row.cost_currency))
				.with(format!(" {}", row.cost_currency), Style::faint()),
			price,
			gain_cell,
			chart,
		]);
	}
	doc.extend(table.render(total_width));

	if !unpriced.is_empty() {
		doc.blank();
		let assets: BTreeSet<(&str, &str)> = unpriced
			.iter()
			.map(|r| (r.asset.as_str(), r.cost_currency.as_str()))
			.collect();
		for (asset, currency) in assets {
			doc.push(
				Line::styled("○ ", Style::faint())
					.with(asset.to_string(), Style::new().bold())
					.with(" has no price yet. Declare one with ", Style::new())
					.with(
						format!("! {as_of} rate {asset} {currency} <price>"),
						Style::color(palette::ACCENT),
					),
			);
		}
	}
	doc
}

fn superscript(n: usize) -> String {
	const DIGITS: [char; 10] =
		['⁰', '¹', '²', '³', '⁴', '⁵', '⁶', '⁷', '⁸', '⁹'];
	n.to_string()
		.chars()
		.map(|c| DIGITS[c.to_digit(10).unwrap_or(0) as usize])
		.collect()
}
