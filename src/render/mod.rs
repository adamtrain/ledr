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

//! Fancy views of reports: color, panels, trees and charts.
//!
//! Every report also has a plain rendering, which lives beside its model in
//! [`crate::reports`] and is what ledr prints with `--plain` or when its
//! output is not a terminal.

pub mod diagnostics;
pub mod entries;
pub mod lots;
pub mod rates;
pub mod register;
pub mod statement;

use crate::ui::style::{ColorLevel, Rgb, Style, palette};
use crate::ui::text::{Doc, Line};
use crate::util::quant::Quant;

/// How output should look
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
	/// Simple text for scripts and pipes, as ledr has always printed
	Plain,
	/// Color, panels and charts, laid out to a width
	Fancy { color: ColorLevel, width: usize },
}

impl Mode {
	pub fn is_fancy(&self) -> bool {
		matches!(self, Mode::Fancy { .. })
	}

	pub fn width(&self) -> usize {
		match self {
			Mode::Plain => 80,
			Mode::Fancy { width, .. } => *width,
		}
	}

	pub fn color(&self) -> ColorLevel {
		match self {
			Mode::Plain => ColorLevel::None,
			Mode::Fancy { color, .. } => *color,
		}
	}

	pub fn render(&self, doc: &Doc) -> String {
		doc.render(self.color())
	}
}

/// Each top-level category has a color, used wherever it appears
pub fn category_color(account: &str) -> Rgb {
	match account.split(':').next().unwrap_or_default() {
		"Assets" => palette::GREEN,
		"Liabilities" => palette::RED,
		"Equity" => palette::PURPLE,
		"Income" => palette::ACCENT,
		"Expenses" => palette::AMBER,
		_ => palette::FAINT,
	}
}

/// An account name with its category tinted and separators faint
pub fn account(name: &str) -> Line {
	let mut line = Line::new();
	for (i, segment) in name.split(':').enumerate() {
		if i == 0 {
			line.push(segment, Style::color(category_color(name)));
		} else {
			line.push(":", Style::faint());
			line.push(segment, Style::new());
		}
	}
	line
}

/// An amount as `1,234.56 USD` with the currency faint and negative
/// numbers tinted, the number padded by `align` if given.
pub fn money(number: &str, currency: &str, negative: bool) -> Line {
	let style = if negative {
		Style::color(palette::RED)
	} else {
		Style::new()
	};
	Line::styled(number, style).with(format!(" {currency}"), Style::faint())
}

/// The sign-aware color for a gain or loss
pub fn gain_color(value: &Quant) -> Rgb {
	if value.is_negative() {
		palette::RED
	} else if value.is_zero() {
		palette::FAINT
	} else {
		palette::GREEN
	}
}

/// `▲ 1,234.00 USD` or `▼ 56.00 USD`, colored
pub fn gain(shown: &str, value: &Quant, currency: &str) -> Line {
	let color = gain_color(value);
	let arrow = if value.is_negative() {
		"▼ "
	} else if value.is_zero() {
		"  "
	} else {
		"▲ "
	};
	let shown = shown.trim_start_matches('-');
	Line::styled(arrow, Style::color(color))
		.with(shown, Style::color(color).bold())
		.with(format!(" {currency}"), Style::faint())
}

/// `n thing` or `n things`, with thousands separators
pub fn plural(n: usize, word: &str) -> String {
	let digits = n.to_string();
	let mut grouped = String::new();
	for (i, c) in digits.chars().enumerate() {
		if i > 0 && (digits.len() - i).is_multiple_of(3) {
			grouped.push(',');
		}
		grouped.push(c);
	}
	if n == 1 {
		return format!("{grouped} {word}");
	}
	let consonant_y = word.len() > 1
		&& word.ends_with('y')
		&& !word[..word.len() - 1].ends_with(['a', 'e', 'i', 'o', 'u']);
	if consonant_y {
		format!("{grouped} {}ies", &word[..word.len() - 1])
	} else {
		format!("{grouped} {word}s")
	}
}

/// A percentage for display, like `23%` or `0.4%`
pub fn percent(fraction: f64) -> String {
	let p = fraction * 100.0;
	if p.abs() >= 10.0 || p == 0.0 {
		format!("{p:.0}%")
	} else {
		format!("{p:.1}%")
	}
}

/// Joined with faint middle dots: `a · b · c`
pub fn dotted(parts: &[String]) -> Line {
	let mut line = Line::new();
	for (i, part) in parts.iter().filter(|p| !p.is_empty()).enumerate() {
		if i > 0 {
			line.push(" · ", Style::faint());
		}
		line.push(part.clone(), Style::faint());
	}
	line
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn test_plural() {
		assert_eq!(plural(1, "entry"), "1 entry");
		assert_eq!(plural(2, "lot"), "2 lots");
		assert_eq!(plural(1234567, "lot"), "1,234,567 lots");
		assert_eq!(plural(3, "entry"), "3 entries");
		assert_eq!(plural(3, "day"), "3 days");
	}

	#[test]
	fn test_percent() {
		assert_eq!(percent(0.2345), "23%");
		assert_eq!(percent(0.004), "0.4%");
		assert_eq!(percent(0.0), "0%");
		assert_eq!(percent(-0.5), "-50%");
	}
}
