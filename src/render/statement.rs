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

//! Balance sheets, income statements and trial balances, drawn as a
//! summary panel over a tree of accounts with proportional bars.

use crate::render::{category_color, dotted, percent};
use crate::reports::statement_reporter::{StatementReporter, StatementRow};
use crate::ui::style::{Rgb, Style, palette};
use crate::ui::text::{Align, Doc, Line, width};
use crate::ui::widgets::{Decimals, badge, bar, panel, split_bar};
use crate::util::quant::Quant;
use std::collections::BTreeMap;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StatementKind {
	Balance,
	Income,
	Trial,
}

impl StatementKind {
	fn title(self) -> &'static str {
		match self {
			StatementKind::Balance => "BALANCE SHEET",
			StatementKind::Income => "INCOME STATEMENT",
			StatementKind::Trial => "TRIAL BALANCE",
		}
	}
}

/// What the statement covers, for its heading
pub struct Heading {
	pub kind: StatementKind,
	/// e.g. "as of 2024-11-30" or "2024-01-01 → 2024-12-31"
	pub period: String,
	/// Faint facts for the panel's border, e.g. the file and entry count
	pub meta: Vec<String>,
	/// Whether amounts were negated with --invert
	pub inverted: bool,
	/// Whether every amount was converted to one currency
	pub collapsed: bool,
}

type Amounts = BTreeMap<String, Quant>;

fn amounts(report: &StatementReporter, category: &str) -> Amounts {
	report
		.category_totals(category)
		.map(|a| a.iter().cloned().collect())
		.unwrap_or_default()
}

fn combine(a: &Amounts, b: &Amounts, scale: i128) -> Amounts {
	let mut out = a.clone();
	for (currency, value) in b {
		*out.entry(currency.clone()).or_default() += *value;
	}
	out.values_mut().for_each(|v| *v = *v * scale);
	out
}

fn signed(amounts: &Amounts, scale: i128) -> Amounts {
	amounts
		.iter()
		.map(|(c, v)| (c.clone(), *v * scale))
		.collect()
}

/// Draws the whole statement: summary panel, then the account tree
pub fn statement(
	report: &StatementReporter,
	heading: &Heading,
	depth: Option<usize>,
	total_width: usize,
) -> Doc {
	let mut doc = Doc::new();
	let rows = report.rows(depth);

	if rows.is_empty() {
		doc.extend(panel(
			vec![
				title(heading, palette::FAINT),
				Line::new(),
				Line::faint("Nothing to show for this period."),
			],
			palette::FAINT,
			Some(dotted(&heading.meta)),
			total_width,
		));
		return doc;
	}

	let (body, color) = summary(report, heading, total_width - 6);
	doc.extend(panel(body, color, Some(dotted(&heading.meta)), total_width));
	doc.blank();
	doc.extend(tree(&rows, total_width));
	doc
}

fn title(heading: &Heading, color: Rgb) -> Line {
	badge(heading.kind.title(), color)
		.with("  ", Style::new())
		.with(heading.period.clone(), Style::faint())
}

/// A label followed by one amount per currency, stacked
fn metric(label: &str, values: &Amounts, color_by_sign: bool) -> Vec<Line> {
	let numbers: Vec<String> = values.values().map(|v| v.to_string()).collect();
	let decimals = Decimals::fit(numbers.iter().map(String::as_str));
	let mut lines = vec![];
	for (i, (currency, value)) in values.iter().enumerate() {
		let label = if i == 0 { label } else { "" };
		let style = if !color_by_sign {
			Style::new().bold()
		} else if value.is_negative() {
			Style::color(palette::RED).bold()
		} else {
			Style::color(palette::GREEN).bold()
		};
		lines.push(
			Line::plain(format!("{label:<12}"))
				.with(decimals.align(&value.to_string()), style)
				.with(format!(" {currency}"), Style::faint()),
		);
	}
	lines
}

fn legend(items: &[(Rgb, String, String)]) -> Line {
	let mut line = Line::new();
	for (i, (color, name, value)) in items.iter().enumerate() {
		if i > 0 {
			line.push("     ", Style::new());
		}
		line.push("● ", Style::color(*color));
		line.push(format!("{name} "), Style::new());
		line.push(value.clone(), Style::color(*color).bold());
	}
	line
}

/// The panel's contents, and the color that sums the statement up
fn summary(
	report: &StatementReporter,
	heading: &Heading,
	inner: usize,
) -> (Vec<Line>, Rgb) {
	let flip = if heading.inverted { -1 } else { 1 };
	let mut body = vec![];

	let color = match heading.kind {
		StatementKind::Balance => {
			let assets = signed(&amounts(report, "Assets"), flip);
			let liabilities = signed(&amounts(report, "Liabilities"), flip);
			let net = combine(&assets, &liabilities, 1);
			let color = mood(&net);
			body.push(title(heading, color));
			body.push(Line::new());
			body.extend(metric("Net worth", &net, true));

			if let Some((currency, _)) = single(&net) {
				let a = assets.get(currency).copied().unwrap_or_default();
				let l = liabilities.get(currency).copied().unwrap_or_default();
				body.push(Line::new());
				body.push(split_bar(
					&[
						(a.abs().to_f64(), palette::GREEN),
						(l.abs().to_f64(), palette::RED),
					],
					inner,
				));
				body.push(legend(&[
					(
						palette::GREEN,
						"Assets".into(),
						format!("{a} {currency}"),
					),
					(
						palette::RED,
						"Liabilities".into(),
						format!("{l} {currency}"),
					),
				]));
			}
			color
		},
		StatementKind::Income => {
			// Uninverted, income is negative and expenses positive
			let earned = signed(&amounts(report, "Income"), -flip);
			let spent = signed(&amounts(report, "Expenses"), flip);
			let net = combine(&earned, &signed(&spent, -1), 1);
			let color = mood(&net);
			body.push(title(heading, color));
			body.push(Line::new());
			body.extend(metric("Net income", &net, true));

			if let Some((currency, net_value)) = single(&net) {
				let earned = earned.get(currency).copied().unwrap_or_default();
				let spent = spent.get(currency).copied().unwrap_or_default();
				let e = earned.to_f64();
				body.push(Line::new());
				if !net_value.is_negative() {
					body.push(split_bar(
						&[
							(spent.to_f64(), palette::AMBER),
							(net_value.to_f64(), palette::GREEN),
						],
						inner,
					));
					let share = |x: f64| {
						if e > 0.0 {
							format!(" ({})", percent(x / e))
						} else {
							String::new()
						}
					};
					body.push(legend(&[
						(
							palette::AMBER,
							"Spent".into(),
							format!(
								"{spent} {currency}{}",
								share(spent.to_f64())
							),
						),
						(
							palette::GREEN,
							"Saved".into(),
							format!(
								"{net_value} {currency}{}",
								share(net_value.to_f64())
							),
						),
					]));
				} else {
					let shortfall = net_value.abs();
					body.push(split_bar(
						&[
							(e.max(0.0), palette::AMBER),
							(shortfall.to_f64(), palette::RED),
						],
						inner,
					));
					let of_income = if e > 0.0 {
						format!(" ({} of income)", percent(spent.to_f64() / e))
					} else {
						String::new()
					};
					body.push(legend(&[
						(
							palette::AMBER,
							"Spent".into(),
							format!("{spent} {currency}{of_income}"),
						),
						(
							palette::RED,
							"Shortfall".into(),
							format!("{shortfall} {currency}"),
						),
					]));
				}
			}
			color
		},
		StatementKind::Trial => {
			let off: Amounts = report
				.totals()
				.iter()
				.filter(|(_, v)| !v.is_zero())
				.cloned()
				.collect();
			let color = if off.is_empty() {
				palette::GREEN
			} else {
				palette::RED
			};
			body.push(title(heading, color));
			body.push(Line::new());
			if off.is_empty() {
				body.push(
					Line::styled("✓ ", Style::color(palette::GREEN).bold())
						.with("Balanced", Style::new().bold())
						.with("  every currency nets to zero", Style::faint()),
				);
			} else {
				body.push(
					Line::styled("✗ ", Style::color(palette::RED).bold())
						.with("Out of balance", Style::new().bold()),
				);
				body.extend(metric("Off by", &off, false));
			}

			let categories =
				["Assets", "Liabilities", "Equity", "Income", "Expenses"];
			let currencies: Vec<String> =
				report.totals().iter().map(|(c, _)| c.clone()).collect();
			if let [currency] = currencies.as_slice() {
				let parts: Vec<(f64, Rgb)> = categories
					.iter()
					.map(|c| {
						let v = amounts(report, c)
							.get(currency)
							.copied()
							.unwrap_or_default();
						(v.abs().to_f64(), category_color(c))
					})
					.collect();
				body.push(Line::new());
				body.push(split_bar(&parts, inner));
				body.push(legend(
					&categories
						.iter()
						.zip(&parts)
						.filter(|(_, (w, _))| *w > 0.0)
						.map(|(c, (_, color))| {
							(*color, c.to_string(), String::new())
						})
						.collect::<Vec<_>>(),
				));
			}
			color
		},
	};

	if !heading.collapsed && distinct_currencies(report) > 1 {
		body.push(Line::new());
		body.push(Line::faint("Tip: -c USD shows everything in one currency"));
	}

	(body, color)
}

fn distinct_currencies(report: &StatementReporter) -> usize {
	report.totals().len()
}

fn single(amounts: &Amounts) -> Option<(&String, Quant)> {
	match amounts.len() {
		1 => amounts.iter().next().map(|(c, v)| (c, *v)),
		_ => None,
	}
}

/// Green when everything is at least zero, red when everything is below,
/// and the accent color when it is a mix
fn mood(amounts: &Amounts) -> Rgb {
	if amounts.values().all(|v| !v.is_negative()) {
		palette::GREEN
	} else if amounts.values().all(|v| v.is_negative()) {
		palette::RED
	} else {
		palette::ACCENT
	}
}

/// The account tree: names with guide lines, aligned amounts, and a bar
/// showing each balance's size relative to the largest in its category.
fn tree(rows: &[StatementRow], total_width: usize) -> Vec<Line> {
	let faint = Style::faint();

	// Scale bars per category and currency
	let mut scale: BTreeMap<(String, String), f64> = BTreeMap::new();
	for row in rows {
		for (currency, value) in &row.amounts {
			let key = (row.category.clone(), currency.clone());
			let v = value.abs().to_f64();
			let entry = scale.entry(key).or_insert(0.0);
			*entry = entry.max(v);
		}
	}

	let numbers: Vec<String> = rows
		.iter()
		.flat_map(|r| r.amounts.iter().map(|(_, v)| v.to_string()))
		.collect();
	let decimals = Decimals::fit(numbers.iter().map(String::as_str));
	let currency_width = rows
		.iter()
		.flat_map(|r| r.amounts.iter().map(|(c, _)| width(c)))
		.max()
		.unwrap_or(0);
	let amount_width = decimals.width() + 1 + currency_width;

	let prefix = |row: &StatementRow,
	              continuation: bool,
	              has_children: bool|
	 -> String {
		if row.depth == 0 {
			return if continuation && has_children {
				"│ ".into()
			} else {
				String::new()
			};
		}
		let mut p = String::new();
		for &more in row.guides.iter().skip(1) {
			p.push_str(if more { "│  " } else { "   " });
		}
		p.push_str(match (continuation, row.is_last) {
			(false, false) => "├─ ",
			(false, true) => "└─ ",
			(true, false) => "│  ",
			(true, true) => "   ",
		});
		if continuation && has_children {
			p.push('│');
		}
		p
	};

	let name_line = |row: &StatementRow| -> Line {
		let mut line = Line::new();
		if row.depth == 0 {
			line.push(
				row.name.clone(),
				Style::color(category_color(&row.category)).bold(),
			);
		} else {
			for (i, segment) in row.name.split(':').enumerate() {
				if i > 0 {
					line.push(":", faint);
				}
				line.push(segment, Style::new());
			}
		}
		if row.hidden > 0 {
			line.push(format!("  +{}", row.hidden), faint);
		}
		line
	};

	let name_width = rows
		.iter()
		.map(|r| width(&prefix(r, false, false)) + name_line(r).width())
		.max()
		.unwrap_or(0)
		.min(total_width * 2 / 5);
	// Bars compare balances in one currency, so they only make sense when
	// the whole statement is in one currency
	let currencies: std::collections::BTreeSet<&String> = rows
		.iter()
		.flat_map(|r| r.amounts.iter().map(|(c, _)| c))
		.collect();
	let bar_cells = total_width
		.saturating_sub(name_width + 2 + amount_width + 2 + 5)
		.min(32);
	let show_bars = bar_cells >= 6 && currencies.len() == 1;

	// Each account's share of its category, e.g. rent's share of expenses
	let category_total = |row: &StatementRow, currency: &str| -> Option<f64> {
		rows.iter()
			.find(|r| r.depth == 0 && r.category == row.category)?
			.amounts
			.iter()
			.find(|(c, _)| c == currency)
			.map(|(_, v)| v.to_f64())
	};

	let mut out = vec![];
	for (i, row) in rows.iter().enumerate() {
		if row.depth == 0 && i > 0 {
			out.push(Line::new());
		}
		let has_children =
			rows.get(i + 1).is_some_and(|next| next.depth > row.depth);

		for (j, (currency, value)) in row.amounts.iter().enumerate() {
			let head = if j == 0 {
				Line::styled(prefix(row, false, has_children), faint)
					.then(name_line(row))
			} else {
				Line::styled(prefix(row, true, has_children), faint)
			};
			let number_style = if row.depth == 0 {
				Style::new().bold()
			} else {
				Style::new()
			};
			let mut line = head
				.fit(name_width, Align::Left)
				.with("  ", Style::new())
				.with(decimals.align(&value.to_string()), number_style)
				.with(format!(" {currency:<currency_width$}"), faint);
			if show_bars {
				let full = scale
					.get(&(row.category.clone(), currency.clone()))
					.copied()
					.unwrap_or(0.0);
				let fraction = if full > 0.0 {
					value.abs().to_f64() / full
				} else {
					0.0
				};
				line.push("  ", Style::new());
				let drawn =
					bar(fraction, bar_cells, category_color(&row.category));
				let pad = bar_cells.saturating_sub(drawn.width());
				line.append(drawn);
				if row.depth > 0 {
					let share = category_total(row, currency)
						.filter(|t| *t != 0.0)
						.map(|t| value.to_f64() / t)
						.filter(|s| (0.0..=1.0).contains(s));
					if let Some(share) = share {
						line.push(" ".repeat(pad + 1), Style::new());
						line.push(format!("{:>4}", percent(share)), faint);
					}
				}
			}
			out.push(line);
		}
	}
	out
}
