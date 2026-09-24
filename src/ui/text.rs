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

//! Lines of styled text that know how wide they are on screen.

use crate::ui::style::{ColorLevel, Style};
use unicode_width::UnicodeWidthStr;

/// On-screen width of plain text, counting wide characters as two columns
pub fn width(text: &str) -> usize {
	UnicodeWidthStr::width(text)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Align {
	Left,
	Right,
	Center,
}

/// One line of text made of differently styled pieces.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Line {
	spans: Vec<(String, Style)>,
}

impl Line {
	pub fn new() -> Self {
		Self::default()
	}

	pub fn styled(text: impl Into<String>, style: Style) -> Self {
		Self::new().with(text, style)
	}

	pub fn plain(text: impl Into<String>) -> Self {
		Self::styled(text, Style::new())
	}

	pub fn faint(text: impl Into<String>) -> Self {
		Self::styled(text, Style::faint())
	}

	/// Appends a piece of text, builder style
	pub fn with(mut self, text: impl Into<String>, style: Style) -> Self {
		self.push(text, style);
		self
	}

	pub fn push(&mut self, text: impl Into<String>, style: Style) {
		let text = text.into();
		if text.is_empty() {
			return;
		}
		match self.spans.last_mut() {
			Some((last, last_style)) if *last_style == style => {
				last.push_str(&text)
			},
			_ => self.spans.push((text, style)),
		}
	}

	pub fn append(&mut self, other: Line) {
		for (text, style) in other.spans {
			self.push(text, style);
		}
	}

	pub fn then(mut self, other: Line) -> Self {
		self.append(other);
		self
	}

	pub fn width(&self) -> usize {
		self.spans.iter().map(|(t, _)| width(t)).sum()
	}

	pub fn is_empty(&self) -> bool {
		self.spans.is_empty()
	}

	/// The text without any styling
	pub fn text(&self) -> String {
		self.spans.iter().map(|(t, _)| t.as_str()).collect()
	}

	/// Pads with spaces to exactly `width` columns, truncating with an
	/// ellipsis if it is too long.
	pub fn fit(self, width: usize, align: Align) -> Line {
		let line = self.truncate(width);
		let gap = width.saturating_sub(line.width());
		let (left, right) = match align {
			Align::Left => (0, gap),
			Align::Right => (gap, 0),
			Align::Center => (gap / 2, gap - gap / 2),
		};
		Line::plain(" ".repeat(left))
			.then(line)
			.with(" ".repeat(right), Style::new())
	}

	/// Shortens to at most `max` columns, ending in an ellipsis if cut
	pub fn truncate(self, max: usize) -> Line {
		if self.width() <= max {
			return self;
		}
		if max == 0 {
			return Line::new();
		}
		let mut out = Line::new();
		let mut used = 0;
		for (text, style) in self.spans {
			let mut piece = String::new();
			for c in text.chars() {
				let w = width(c.encode_utf8(&mut [0; 4]));
				if used + w > max - 1 {
					break;
				}
				piece.push(c);
				used += w;
			}
			out.push(piece, style);
			if used >= max - 1 {
				break;
			}
		}
		let last_style = out.spans.last().map(|(_, s)| *s).unwrap_or_default();
		out.push("…", last_style);
		out
	}

	pub fn render(&self, level: ColorLevel) -> String {
		self.spans
			.iter()
			.map(|(text, style)| level.paint(text, *style))
			.collect()
	}
}

/// Styled text in many lines, rendered together
#[derive(Clone, Debug, Default)]
pub struct Doc {
	pub lines: Vec<Line>,
}

impl From<Vec<Line>> for Doc {
	fn from(lines: Vec<Line>) -> Self {
		Self { lines }
	}
}

impl Doc {
	pub fn new() -> Self {
		Self::default()
	}

	pub fn push(&mut self, line: Line) {
		self.lines.push(line);
	}

	pub fn blank(&mut self) {
		self.lines.push(Line::new());
	}

	pub fn extend(&mut self, lines: impl IntoIterator<Item = Line>) {
		self.lines.extend(lines);
	}

	pub fn render(&self, level: ColorLevel) -> String {
		let mut out = String::new();
		for line in &self.lines {
			out.push_str(line.render(level).trim_end_matches(' '));
			out.push('\n');
		}
		out
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::ui::style::palette;

	#[test]
	fn test_width_counts_columns() {
		assert_eq!(width("abc"), 3);
		assert_eq!(width("━━"), 2);
		assert_eq!(width("日本"), 4);
		let line = Line::plain("ab").with("cd", Style::faint());
		assert_eq!(line.width(), 4);
		assert_eq!(line.text(), "abcd");
	}

	#[test]
	fn test_fit_and_truncate() {
		let line = Line::plain("hello");
		assert_eq!(line.clone().fit(8, Align::Left).text(), "hello   ");
		assert_eq!(line.clone().fit(8, Align::Right).text(), "   hello");
		assert_eq!(line.clone().fit(9, Align::Center).text(), "  hello  ");
		assert_eq!(line.clone().fit(4, Align::Left).text(), "hel…");
		assert_eq!(line.truncate(1).text(), "…");
	}

	#[test]
	fn test_adjacent_spans_with_one_style_merge() {
		let style = Style::color(palette::GREEN);
		let line = Line::styled("a", style).with("b", style);
		assert_eq!(
			line.render(ColorLevel::TrueColor)
				.matches("\x1b[0m")
				.count(),
			1
		);
	}
}
