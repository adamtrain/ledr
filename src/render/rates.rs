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

//! Exchange rates, one line per pair of currencies with a price history.

use crate::gl::observed_rate::{ObservationType, ObservedRate};
use crate::render::plural;
use crate::ui::style::{Rgb, Style, palette};
use crate::ui::text::{Doc, Line};
use crate::ui::widgets::{Column, Decimals, Table, rule, sparkline};
use crate::util::quant::Quant;
use std::collections::{BTreeMap, BTreeSet};

fn kind(t: ObservationType) -> (&'static str, &'static str, Rgb) {
	match t {
		ObservationType::Declared => ("●", "declared", palette::GREEN),
		ObservationType::Direct => ("◐", "observed", palette::ACCENT),
		ObservationType::Inferred => ("○", "inferred", palette::AMBER),
	}
}

/// A rate with enough significant digits to be useful: 115,420.00, 1.2176,
/// 0.000021
fn readable(rate: Quant) -> String {
	let mut rate = rate;
	let magnitude = rate.abs().to_f64();
	let places = if magnitude >= 1000.0 {
		2
	} else if magnitude >= 1.0 {
		4
	} else {
		0
	};
	rate.set_render_precision(places.min(2), true);
	if places == 0 {
		rate.make_visible();
		// Two more significant digits than the first visible one
		let visible = rate.render_precision();
		return format!("{rate:.*}", (visible + 2) as usize);
	}
	format!("{rate:.*}", places as usize)
}

pub fn rates(
	rates: &BTreeMap<(String, String), Vec<ObservedRate>>,
	total_width: usize,
) -> Doc {
	let mut doc = Doc::new();
	if rates.is_empty() {
		doc.push(Line::faint(
			"No exchange rates: every entry uses a single currency.",
		));
		return doc;
	}

	// Each directly connected pair once, facing whichever way gives a rate
	// of at least one, e.g. 1 BTC = 50,000 USD rather than 1 USD = 0.00002
	// BTC. Pairs only connected through others follow from these.
	let (direct, indirect): (Vec<_>, Vec<_>) = rates
		.iter()
		.filter(|((a, b), _)| a < b)
		.partition(|(_, history)| history.iter().any(|o| o.direct));
	let pairs: BTreeSet<(&String, &String)> =
		direct.iter().map(|((a, b), _)| (a, b)).collect();

	let mut rows = vec![];
	for (a, b) in pairs {
		let forward = &rates[&(a.clone(), b.clone())];
		let Some(latest) = forward.first() else {
			continue;
		};
		let (base, quote, history) = if latest.rate >= 1 {
			(a, b, forward)
		} else {
			(b, a, &rates[&(b.clone(), a.clone())])
		};
		rows.push((base, quote, history));
	}

	let numbers: Vec<String> =
		rows.iter().map(|(_, _, h)| readable(h[0].rate)).collect();
	let decimals = Decimals::fit(numbers.iter().map(String::as_str));

	doc.push(rule("Exchange rates", total_width));
	let mut table = Table::new(vec![
		Column::left("Pair"),
		Column::right("Rate"),
		Column::left("Kind"),
		Column::left("As of"),
		Column::left("History"),
	]);
	for ((base, quote, history), number) in rows.iter().zip(&numbers) {
		let latest = &history[0];
		let (glyph, name, color) = kind(latest.observation_type);

		// Oldest to newest, one point per dated observation
		let points: Vec<f64> = history
			.iter()
			.rev()
			.filter(|o| o.date.is_some() && o.direct)
			.map(|o| o.rate.to_f64())
			.collect();
		let chart = if points.len() > 1 {
			Line::styled(sparkline(&points, 16), Style::color(palette::ACCENT))
				.with(
					format!(" {}", plural(points.len(), "date")),
					Style::faint(),
				)
		} else {
			Line::new()
		};

		table.row(vec![
			Line::plain(format!("1 {base}"))
				.with(" → ", Style::faint())
				.with((*quote).clone(), Style::new().bold()),
			Line::plain(decimals.align(number)),
			Line::styled(format!("{glyph} "), Style::color(color))
				.with(name, Style::faint()),
			match latest.date {
				Some(date) => Line::faint(date.to_string()),
				None => Line::faint("across dates"),
			},
			chart,
		]);
	}
	doc.extend(table.render(total_width));
	if !indirect.is_empty() {
		doc.push(Line::faint(format!(
			"+ {} between currencies that only meet through others; --plain lists them all",
			plural(indirect.len(), "more pair")
		)));
	}
	doc.blank();
	doc.push(
		Line::styled("● ", Style::color(palette::GREEN))
			.with("declared in a rate directive   ", Style::faint())
			.with("◐ ", Style::color(palette::ACCENT))
			.with("observed in an entry   ", Style::faint())
			.with("○ ", Style::color(palette::AMBER))
			.with("inferred through other currencies", Style::faint()),
	);
	doc
}
