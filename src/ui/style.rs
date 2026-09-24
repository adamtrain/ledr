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

//! Colors and text styles, and how to write them to a terminal.

use std::io::IsTerminal;

/// A 24-bit color
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rgb(pub u8, pub u8, pub u8);

impl Rgb {
	/// The nearest color in the xterm 256-color palette's 6×6×6 cube, or its
	/// greyscale ramp, for terminals without 24-bit color.
	fn to_ansi256(self) -> u8 {
		let Rgb(r, g, b) = self;
		let max = r.max(g).max(b);
		let min = r.min(g).min(b);
		if max - min < 12 {
			// Grey: 24 steps from 8 to 238
			if r < 4 {
				return 16;
			}
			if r > 246 {
				return 231;
			}
			return 232 + ((r as u16 - 8) * 24 / 247) as u8;
		}
		let level = |c: u8| -> u8 {
			match c {
				0..=47 => 0,
				48..=114 => 1,
				_ => ((c as u16 - 35) / 40) as u8,
			}
		};
		16 + 36 * level(r) + 6 * level(g) + level(b)
	}
}

/// ledr's colors, shared with its sibling tool panc so they feel related.
pub mod palette {
	use super::Rgb;

	pub const ACCENT: Rgb = Rgb(0x7c, 0x83, 0xf7);
	pub const GREEN: Rgb = Rgb(0x1f, 0xbf, 0x8f);
	pub const AMBER: Rgb = Rgb(0xeb, 0x9a, 0x12);
	pub const RED: Rgb = Rgb(0xf2, 0x50, 0x6e);
	pub const PURPLE: Rgb = Rgb(0xa8, 0x71, 0xf7);
	pub const TEAL: Rgb = Rgb(0x56, 0xb6, 0xc2);
	pub const FAINT: Rgb = Rgb(0x6c, 0x6c, 0x6c);
	/// Text on a colored badge
	pub const INK: Rgb = Rgb(0x11, 0x11, 0x11);
}

/// How a piece of text should look
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Style {
	pub fg: Option<Rgb>,
	pub bg: Option<Rgb>,
	pub bold: bool,
	pub italic: bool,
	pub underline: bool,
}

impl Style {
	pub const fn new() -> Self {
		Self {
			fg: None,
			bg: None,
			bold: false,
			italic: false,
			underline: false,
		}
	}

	pub const fn fg(mut self, color: Rgb) -> Self {
		self.fg = Some(color);
		self
	}

	pub const fn on(mut self, color: Rgb) -> Self {
		self.bg = Some(color);
		self
	}

	pub const fn bold(mut self) -> Self {
		self.bold = true;
		self
	}

	pub const fn italic(mut self) -> Self {
		self.italic = true;
		self
	}

	pub const fn underline(mut self) -> Self {
		self.underline = true;
		self
	}

	pub const fn faint() -> Self {
		Self::new().fg(palette::FAINT)
	}

	pub const fn color(color: Rgb) -> Self {
		Self::new().fg(color)
	}

	pub fn is_plain(&self) -> bool {
		*self == Self::default()
	}
}

/// How much color the terminal can show
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ColorLevel {
	None,
	Ansi256,
	TrueColor,
}

impl ColorLevel {
	/// Detects what stdout supports, honoring the NO_COLOR convention.
	pub fn detect() -> Self {
		if std::env::var_os("NO_COLOR").is_some_and(|v| !v.is_empty()) {
			return Self::None;
		}
		if std::env::var("TERM").is_ok_and(|t| t == "dumb") {
			return Self::None;
		}
		match std::env::var("COLORTERM") {
			Ok(v) if v == "truecolor" || v == "24bit" => Self::TrueColor,
			_ => Self::Ansi256,
		}
	}

	/// Wraps text in the escape codes for a style
	pub fn paint(self, text: &str, style: Style) -> String {
		if self == Self::None || style.is_plain() || text.is_empty() {
			return text.to_string();
		}

		let mut codes: Vec<String> = vec![];
		if style.bold {
			codes.push("1".into());
		}
		if style.italic {
			codes.push("3".into());
		}
		if style.underline {
			codes.push("4".into());
		}
		for (color, layer) in [(style.fg, 38), (style.bg, 48)] {
			let Some(color) = color else { continue };
			codes.push(match self {
				Self::TrueColor => {
					format!("{layer};2;{};{};{}", color.0, color.1, color.2)
				},
				_ => format!("{layer};5;{}", color.to_ansi256()),
			});
		}
		format!("\x1b[{}m{text}\x1b[0m", codes.join(";"))
	}
}

/// Whether stdout is an interactive terminal
pub fn stdout_is_terminal() -> bool {
	std::io::stdout().is_terminal()
}

/// The width to lay fancy output out in: the terminal's, within reason.
pub fn terminal_width() -> usize {
	const DEFAULT: usize = 100;
	const MAX: usize = 120;
	let width = terminal_size::terminal_size()
		.map(|(w, _)| w.0 as usize)
		.or_else(|| std::env::var("COLUMNS").ok()?.parse().ok())
		.unwrap_or(DEFAULT);
	width.clamp(40, MAX)
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn test_paint() {
		let style = Style::color(Rgb(255, 0, 0)).bold();
		assert_eq!(ColorLevel::None.paint("x", style), "x");
		assert_eq!(
			ColorLevel::TrueColor.paint("x", style),
			"\x1b[1;38;2;255;0;0mx\x1b[0m"
		);
		assert_eq!(
			ColorLevel::Ansi256.paint("x", style),
			"\x1b[1;38;5;196mx\x1b[0m"
		);
		assert_eq!(ColorLevel::TrueColor.paint("x", Style::new()), "x");
	}

	#[test]
	fn test_ansi256_greys() {
		assert_eq!(Rgb(0, 0, 0).to_ansi256(), 16);
		assert_eq!(Rgb(255, 255, 255).to_ansi256(), 231);
		let grey = palette::FAINT.to_ansi256();
		assert!((232..=255).contains(&grey));
	}
}
