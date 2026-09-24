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

//! Friendly ways of writing dates and periods on the command line.

use crate::util::date::Date;
use anyhow::{Error, anyhow, bail};

/// Which end of a range a date bounds. `2024` means January 1 as a start,
/// and December 31 as an end.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Edge {
	Start,
	End,
}

/// Parses a date that bounds a range: `2024-03-15`, `2024-03`, `2024`,
/// `today`, `yesterday`, `tomorrow`, or a number of days ago like `-30`.
pub fn parse_bound(text: &str, edge: Edge, today: Date) -> Result<Date, Error> {
	let (start, end) = parse_period(text, today).map_err(|_| {
		anyhow!(
			"`{text}` is not a date I understand; try 2024-03-15, 2024-03, \
			2024, today or yesterday"
		)
	})?;
	Ok(match edge {
		Edge::Start => start,
		Edge::End => end,
	})
}

/// Parses a period of time into its first and last days: any date that
/// [`parse_bound`] accepts (a single day, month or year), a quarter like
/// `2024-Q1`, or a relative period: `this-month`, `last-month`,
/// `this-quarter`, `last-quarter`, `this-year`, `last-year`, `ytd`, `mtd`.
pub fn parse_period(text: &str, today: Date) -> Result<(Date, Date), Error> {
	let text = text.trim().to_lowercase();
	let year_start = |y: u32| Date::new(y, 1, 1);
	let year_end = |y: u32| Date::new(y, 12, 31);
	let month = |y: u32, m: u8| -> Result<(Date, Date), Error> {
		let start = Date::new(y, m, 1)?;
		Ok((start, start.end_of_month()))
	};
	let quarter = |y: u32, q: u8| -> Result<(Date, Date), Error> {
		if !(1..=4).contains(&q) {
			bail!("quarters are Q1 to Q4");
		}
		let (start, _) = month(y, q * 3 - 2)?;
		let (_, end) = month(y, q * 3)?;
		Ok((start, end))
	};
	let previous_month = |d: Date| {
		if d.month() == 1 {
			(d.year() - 1, 12)
		} else {
			(d.year(), d.month() - 1)
		}
	};
	let this_quarter = (today.month() - 1) / 3 + 1;

	let day = |d: Date| Ok((d, d));
	match text.as_str() {
		"today" => return day(today),
		"yesterday" => return day(today.add_days(-1)),
		"tomorrow" => return day(today.add_days(1)),
		"this-month" | "thismonth" | "mtd" => {
			return month(today.year(), today.month())
				.map(|(s, e)| (s, if text == "mtd" { today } else { e }));
		},
		"last-month" | "lastmonth" => {
			let (y, m) = previous_month(today);
			return month(y, m);
		},
		"this-quarter" | "thisquarter" => {
			return quarter(today.year(), this_quarter);
		},
		"last-quarter" | "lastquarter" => {
			return if this_quarter == 1 {
				quarter(today.year() - 1, 4)
			} else {
				quarter(today.year(), this_quarter - 1)
			};
		},
		"this-year" | "thisyear" => {
			return Ok((year_start(today.year())?, year_end(today.year())?));
		},
		"last-year" | "lastyear" => {
			let y = today.year() - 1;
			return Ok((year_start(y)?, year_end(y)?));
		},
		"ytd" => return Ok((year_start(today.year())?, today)),
		_ => {},
	}

	// A number of days ago, like -30
	if let Some(days) = text.strip_prefix('-')
		&& let Ok(days) = days.parse::<i64>()
	{
		return day(today.add_days(-days));
	}

	let parts: Vec<&str> = text.split('-').collect();
	let number = |s: &str| -> Result<u32, Error> {
		if s.is_empty() || !s.bytes().all(|b| b.is_ascii_digit()) {
			bail!("not a number");
		}
		Ok(s.parse()?)
	};
	match parts.as_slice() {
		[y] => {
			let y = number(y)?;
			Ok((year_start(y)?, year_end(y)?))
		},
		[y, q] if q.starts_with('q') => {
			quarter(number(y)?, number(&q[1..])? as u8)
		},
		[y, m] => month(number(y)?, number(m)?.try_into()?),
		[y, m, d] => day(Date::new(
			number(y)?,
			number(m)?.try_into()?,
			number(d)?.try_into()?,
		)?),
		_ => bail!("unrecognized period `{text}`"),
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	fn d(s: &str) -> Date {
		Date::from_str(s).unwrap()
	}

	fn period(s: &str) -> (String, String) {
		let (a, b) = parse_period(s, d("2026-05-17")).unwrap();
		(a.to_string(), b.to_string())
	}

	#[test]
	fn test_absolute_periods() {
		assert_eq!(period("2024"), ("2024-01-01".into(), "2024-12-31".into()));
		assert_eq!(
			period("2024-02"),
			("2024-02-01".into(), "2024-02-29".into())
		);
		assert_eq!(
			period("2023-02"),
			("2023-02-01".into(), "2023-02-28".into())
		);
		assert_eq!(
			period("2024-3-5"),
			("2024-03-05".into(), "2024-03-05".into())
		);
		assert_eq!(
			period("2024-Q2"),
			("2024-04-01".into(), "2024-06-30".into())
		);
	}

	#[test]
	fn test_relative_periods() {
		assert_eq!(period("today"), ("2026-05-17".into(), "2026-05-17".into()));
		assert_eq!(period("yesterday").0, "2026-05-16");
		assert_eq!(period("-30").0, "2026-04-17");
		assert_eq!(
			period("this-month"),
			("2026-05-01".into(), "2026-05-31".into())
		);
		assert_eq!(period("mtd"), ("2026-05-01".into(), "2026-05-17".into()));
		assert_eq!(
			period("last-month"),
			("2026-04-01".into(), "2026-04-30".into())
		);
		assert_eq!(
			period("this-quarter"),
			("2026-04-01".into(), "2026-06-30".into())
		);
		assert_eq!(
			period("last-quarter"),
			("2026-01-01".into(), "2026-03-31".into())
		);
		assert_eq!(
			period("last-year"),
			("2025-01-01".into(), "2025-12-31".into())
		);
		assert_eq!(period("ytd"), ("2026-01-01".into(), "2026-05-17".into()));
		let jan = parse_period("last-month", d("2026-01-10")).unwrap();
		assert_eq!(jan.0.to_string(), "2025-12-01");
	}

	#[test]
	fn test_bounds() {
		let today = d("2026-05-17");
		assert_eq!(
			parse_bound("2024", Edge::Start, today).unwrap(),
			d("2024-01-01")
		);
		assert_eq!(
			parse_bound("2024", Edge::End, today).unwrap(),
			d("2024-12-31")
		);
		assert_eq!(
			parse_bound("2024-11", Edge::End, today).unwrap(),
			d("2024-11-30")
		);
		for bad in ["", "soon", "2024-13", "2024-02-30", "2024-Q5", "20x4"] {
			assert!(parse_bound(bad, Edge::Start, today).is_err(), "{bad}");
		}
	}
}
