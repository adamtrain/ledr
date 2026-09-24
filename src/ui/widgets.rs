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

//! Building blocks for fancy output: panels, badges, bars and tables.

use crate::ui::style::{Rgb, Style, palette};
use crate::ui::text::{Align, Line, width};

/// Bold dark text on a solid color, e.g. ` BALANCE SHEET `
pub fn badge(text: &str, color: Rgb) -> Line {
	Line::styled(
		format!(" {text} "),
		Style::color(palette::INK).on(color).bold(),
	)
}

/// A faint section heading with a line trailing off to the right
pub fn rule(title: &str, total_width: usize) -> Line {
	let used = width(title) + 1;
	Line::faint(title).with(
		format!(" {}", "─".repeat(total_width.saturating_sub(used + 1))),
		Style::faint(),
	)
}

/// A box with rounded corners around `body`, with an optional note set into
/// its bottom border, like panc's verdict panel.
pub fn panel(
	body: Vec<Line>,
	color: Rgb,
	subtitle: Option<Line>,
	total_width: usize,
) -> Vec<Line> {
	let border = Style::color(color);
	let inner = total_width.saturating_sub(2);
	let content = inner.saturating_sub(4);

	let mut out =
		vec![Line::styled(format!("╭{}╮", "─".repeat(inner)), border)];
	let blank = Line::styled("│", border)
		.with(" ".repeat(inner), Style::new())
		.with("│", border);
	out.push(blank.clone());
	for line in body {
		out.push(
			Line::styled("│", border)
				.with("  ", Style::new())
				.then(line.fit(content, Align::Left))
				.with("  ", Style::new())
				.with("│", border),
		);
	}
	out.push(blank);

	let bottom = match subtitle {
		Some(note) => {
			let note = Line::plain(" ").then(note).with(" ", Style::new());
			let note = note.truncate(inner.saturating_sub(4));
			let left = inner.saturating_sub(note.width() + 2);
			Line::styled(format!("╰{}", "─".repeat(left)), border)
				.then(note)
				.with("──╯", border)
		},
		None => Line::styled(format!("╰{}╯", "─".repeat(inner)), border),
	};
	out.push(bottom);
	out
}

/// A horizontal bar `cells` wide at most, filled in proportion. Anything
/// above zero shows at least a sliver, so small values do not vanish.
pub fn bar(fraction: f64, cells: usize, color: Rgb) -> Line {
	let fraction = if fraction.is_finite() {
		fraction.clamp(0.0, 1.0)
	} else {
		0.0
	};
	let filled = (fraction * cells as f64).round() as usize;
	if filled == 0 && fraction > 0.0 && cells > 0 {
		return Line::styled("╸", Style::color(color));
	}
	Line::styled("━".repeat(filled), Style::color(color))
}

/// A bar with a faint track showing the unfilled part, like a gauge
pub fn meter(fraction: f64, cells: usize, color: Rgb) -> Line {
	let filled = bar(fraction, cells, color);
	let rest = cells.saturating_sub(filled.width());
	filled.with("─".repeat(rest), Style::faint())
}

/// One bar split into colored segments in proportion to their weights,
/// e.g. how much of an income went to spending versus saving.
pub fn split_bar(parts: &[(f64, Rgb)], cells: usize) -> Line {
	let weights: Vec<f64> = parts
		.iter()
		.map(|(w, _)| if w.is_finite() { w.max(0.0) } else { 0.0 })
		.collect();
	let total: f64 = weights.iter().sum();
	if total <= 0.0 {
		return Line::faint("━".repeat(cells));
	}

	// Largest remainder, so the segments always add up to exactly `cells`
	let exact: Vec<f64> =
		weights.iter().map(|w| w / total * cells as f64).collect();
	let mut counts: Vec<usize> =
		exact.iter().map(|e| e.floor() as usize).collect();
	let mut order: Vec<usize> = (0..parts.len()).collect();
	order.sort_by(|&a, &b| {
		(exact[b] - exact[b].floor()).total_cmp(&(exact[a] - exact[a].floor()))
	});
	let assigned: usize = counts.iter().sum();
	for &i in order.iter().take(cells.saturating_sub(assigned)) {
		counts[i] += 1;
	}

	let mut line = Line::new();
	for ((_, color), count) in parts.iter().zip(counts) {
		line.push("━".repeat(count), Style::color(*color));
	}
	line
}

/// A tiny chart of how values changed over time, e.g. ▁▂▄▆█▇▅
pub fn sparkline(values: &[f64], cells: usize) -> String {
	const TICKS: [char; 8] = ['▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];
	if values.is_empty() || cells == 0 {
		return String::new();
	}

	// Each cell shows the last value in its stretch of time
	let sampled: Vec<f64> = if values.len() <= cells {
		values.to_vec()
	} else {
		(1..=cells)
			.map(|i| values[(i * values.len()).div_ceil(cells) - 1])
			.collect()
	};

	let min = sampled.iter().copied().fold(f64::INFINITY, f64::min);
	let max = sampled.iter().copied().fold(f64::NEG_INFINITY, f64::max);
	sampled
		.iter()
		.map(|v| {
			if (max - min).abs() < f64::EPSILON {
				TICKS[3]
			} else {
				let level = ((v - min) / (max - min) * 7.0).round() as usize;
				TICKS[level.min(7)]
			}
		})
		.collect()
}

/// Lines numbers up on their decimal points, e.g. for a column of amounts
/// in currencies with different numbers of decimal places.
#[derive(Clone, Copy, Debug, Default)]
pub struct Decimals {
	whole: usize,
	fraction: usize,
}

impl Decimals {
	pub fn fit<'a>(numbers: impl IntoIterator<Item = &'a str>) -> Self {
		numbers.into_iter().fold(Self::default(), |acc, n| {
			let (whole, fraction) = split_decimal(n);
			Self {
				whole: acc.whole.max(width(whole)),
				fraction: acc
					.fraction
					.max(fraction.map_or(0, |f| width(f) + 1)),
			}
		})
	}

	pub fn width(&self) -> usize {
		self.whole + self.fraction
	}

	/// Pads a number so that its decimal point lands in the column's
	pub fn align(&self, number: &str) -> String {
		let (whole, fraction) = split_decimal(number);
		let fraction = fraction.map_or(String::new(), |f| format!(".{f}"));
		format!(
			"{}{whole}{fraction}{}",
			" ".repeat(self.whole.saturating_sub(width(whole))),
			" ".repeat(self.fraction.saturating_sub(width(&fraction)))
		)
	}
}

fn split_decimal(number: &str) -> (&str, Option<&str>) {
	match number.split_once('.') {
		Some((whole, fraction)) => (whole, Some(fraction)),
		None => (number, None),
	}
}

/// A column in a [`Table`]
#[derive(Clone, Debug)]
pub struct Column {
	pub header: String,
	pub align: Align,
	/// Whether this column gives up width when the table is too wide
	pub shrinks: bool,
}

impl Column {
	pub fn left(header: &str) -> Self {
		Self {
			header: header.into(),
			align: Align::Left,
			shrinks: false,
		}
	}

	pub fn right(header: &str) -> Self {
		Self {
			align: Align::Right,
			..Self::left(header)
		}
	}

	pub fn shrinking(mut self) -> Self {
		self.shrinks = true;
		self
	}
}

#[derive(Clone, Debug)]
enum Row {
	Cells(Vec<Line>),
	/// A faint rule under the given columns, e.g. above a total
	Rule(Vec<usize>),
}

/// A borderless table with a faint header, as in panc's segment table.
#[derive(Clone, Debug, Default)]
pub struct Table {
	columns: Vec<Column>,
	rows: Vec<Row>,
	gap: usize,
}

impl Table {
	pub fn new(columns: Vec<Column>) -> Self {
		Self {
			columns,
			rows: vec![],
			gap: 2,
		}
	}

	pub fn row(&mut self, cells: Vec<Line>) {
		self.rows.push(Row::Cells(cells));
	}

	pub fn rule_under(&mut self, columns: Vec<usize>) {
		self.rows.push(Row::Rule(columns));
	}

	pub fn is_empty(&self) -> bool {
		self.rows.is_empty()
	}

	pub fn render(&self, max_width: usize) -> Vec<Line> {
		let count = self.columns.len();
		let mut widths: Vec<usize> =
			self.columns.iter().map(|c| width(&c.header)).collect();
		for row in &self.rows {
			if let Row::Cells(cells) = row {
				for (i, cell) in cells.iter().enumerate().take(count) {
					widths[i] = widths[i].max(cell.width());
				}
			}
		}

		// Give up width from shrinking columns until the table fits
		let total = |w: &[usize]| {
			w.iter().sum::<usize>() + self.gap * count.saturating_sub(1)
		};
		let mut excess = total(&widths).saturating_sub(max_width);
		for (i, column) in self.columns.iter().enumerate() {
			if excess == 0 {
				break;
			}
			if column.shrinks {
				let give = excess.min(widths[i].saturating_sub(8));
				widths[i] -= give;
				excess -= give;
			}
		}

		let gap = " ".repeat(self.gap);
		let join = |cells: Vec<Line>| {
			let mut line = Line::new();
			for (i, cell) in cells.into_iter().enumerate() {
				if i > 0 {
					line.push(gap.clone(), Style::new());
				}
				line.append(cell);
			}
			line
		};

		let mut out = vec![];
		let has_header = self.columns.iter().any(|c| !c.header.is_empty());
		if has_header {
			out.push(join(
				self.columns
					.iter()
					.enumerate()
					.map(|(i, c)| {
						Line::faint(c.header.clone()).fit(widths[i], c.align)
					})
					.collect(),
			));
		}
		for row in &self.rows {
			out.push(match row {
				Row::Cells(cells) => join(
					(0..count)
						.map(|i| {
							cells
								.get(i)
								.cloned()
								.unwrap_or_default()
								.fit(widths[i], self.columns[i].align)
						})
						.collect(),
				),
				Row::Rule(columns) => join(
					(0..count)
						.map(|i| {
							if columns.contains(&i) {
								Line::faint("─".repeat(widths[i]))
							} else {
								Line::plain(" ".repeat(widths[i]))
							}
						})
						.collect(),
				),
			});
		}
		out
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn test_bars() {
		assert_eq!(bar(0.5, 10, palette::GREEN).text(), "━━━━━");
		assert_eq!(bar(0.001, 10, palette::GREEN).text(), "╸");
		assert_eq!(bar(0.0, 10, palette::GREEN).text(), "");
		assert_eq!(bar(2.0, 4, palette::GREEN).text(), "━━━━");
		assert_eq!(meter(0.5, 4, palette::GREEN).text(), "━━──");
	}

	#[test]
	fn test_split_bar_always_fills_exactly() {
		for cells in [1, 7, 10, 33] {
			let line = split_bar(
				&[
					(1.0, palette::GREEN),
					(1.0, palette::RED),
					(1.0, palette::AMBER),
				],
				cells,
			);
			assert_eq!(line.width(), cells);
		}
	}

	#[test]
	fn test_sparkline() {
		assert_eq!(sparkline(&[0.0, 7.0], 10), "▁█");
		assert_eq!(sparkline(&[1.0, 1.0, 1.0], 10), "▄▄▄");
		assert_eq!(
			sparkline(&(0..100).map(f64::from).collect::<Vec<_>>(), 8)
				.chars()
				.count(),
			8
		);
	}

	#[test]
	fn test_decimals_align() {
		let d = Decimals::fit(["1,234.5", "12", "0.001"]);
		assert_eq!(d.align("1,234.5"), "1,234.5  ");
		assert_eq!(d.align("12"), "   12    ");
		assert_eq!(d.align("0.001"), "    0.001");
		assert_eq!(d.width(), 9);
	}

	#[test]
	fn test_panel_is_exactly_as_wide_as_asked() {
		let lines = panel(
			vec![Line::plain("hello")],
			palette::ACCENT,
			Some(Line::faint("note")),
			30,
		);
		assert!(lines.iter().all(|l| l.width() == 30), "{lines:#?}");
		assert!(lines.last().unwrap().text().contains(" note "));
	}

	#[test]
	fn test_table_shrinks_to_fit() {
		let mut table = Table::new(vec![
			Column::left("Name").shrinking(),
			Column::right("Amount"),
		]);
		table.row(vec![
			Line::plain("a very long description indeed"),
			Line::plain("1.00"),
		]);
		let lines = table.render(20);
		assert!(lines.iter().all(|l| l.width() <= 20));
		assert!(lines[1].text().contains('…'));
	}
}
