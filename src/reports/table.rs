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
use std::fmt::Write;

/// Standard table printer for those reports, such as account summaries, that
/// report a potentially large number of single-line objects.
///
/// Not for use with financial statements that require complex nesting, sorting
/// and spacing.
pub struct Table {
	column_count: usize,
	rows: Vec<Row>,
	right_align: Vec<bool>, // indicates columns by index
}

pub enum Row {
	Header(Vec<String>),
	Data(Vec<String>),
	Separator,
	PartialSeparator(Vec<bool>), // indicates columns by index
}

impl Table {
	pub fn new(column_count: usize) -> Self {
		Self {
			column_count,
			rows: Vec::new(),
			right_align: vec![false; column_count],
		}
	}

	/// Adds a header row.
	pub fn add_header(&mut self, row: Vec<&str>) {
		self.rows.push(Row::Header(
			row.into_iter().map(|s| s.to_string()).collect(),
		));
	}

	/// Adds a data row.
	pub fn add_row(&mut self, row: Vec<&str>) {
		self.rows
			.push(Row::Data(row.into_iter().map(|s| s.to_string()).collect()));
	}

	/// Adds a full separator row.
	pub fn add_separator(&mut self) {
		self.rows.push(Row::Separator);
	}

	/// Adds a partial separator row for selected columns.
	pub fn add_partial_separator(&mut self, indices: Vec<usize>) {
		let mut cols = vec![false; self.column_count];
		for i in indices {
			cols[i] = true;
		}
		self.rows.push(Row::PartialSeparator(cols));
	}

	/// Specifies columns that should be right-aligned by index.
	pub fn right_align(&mut self, cols: Vec<usize>) {
		for col in cols {
			self.right_align[col] = true;
		}
	}

	/// Renders the table, preceded by a blank line
	pub fn render(&self) -> String {
		let mut out = String::from("\n");
		let mut max_widths = vec![0; self.column_count];

		// Calculate maximum column widths for proper spacing
		for row in &self.rows {
			if let Row::Data(data_row) | Row::Header(data_row) = row {
				for (i, value) in data_row.iter().enumerate() {
					max_widths[i] = max_widths[i].max(value.len());
				}
			}
		}

		// Render the table
		for row in &self.rows {
			match row {
				Row::Header(header_row) => self.print_centered_row(
					&mut out,
					&max_widths,
					header_row,
					" | ",
				),
				Row::Data(data_row) => {
					self.print_data_row(&mut out, &max_widths, data_row, "   ")
				},
				Row::Separator => self.print_separator(&mut out, &max_widths),
				Row::PartialSeparator(data_sep) => self
					.print_partial_separator(&mut out, &max_widths, data_sep),
			}
		}
		out
	}

	fn print_data_row(
		&self,
		out: &mut String,
		max_widths: &[usize],
		data_row: &[String],
		separator: &str,
	) {
		for (i, value) in data_row.iter().enumerate() {
			if self.right_align[i] {
				let _ = write!(out, "{:>width$}", value, width = max_widths[i]);
			} else {
				let _ = write!(out, "{:<width$}", value, width = max_widths[i]);
			}
			if i < data_row.len() - 1 {
				let _ = write!(out, "{separator}");
			}
		}
		out.push('\n');
	}

	fn print_centered_row(
		&self,
		out: &mut String,
		max_widths: &[usize],
		data_row: &[String],
		separator: &str,
	) {
		for (i, value) in data_row.iter().enumerate() {
			let centered_value = Table::center_align(value, max_widths[i]);
			let _ = write!(out, "{centered_value}");
			if i < data_row.len() - 1 {
				let _ = write!(out, "{separator}");
			}
		}
		out.push('\n');
	}

	fn print_separator(&self, out: &mut String, max_widths: &[usize]) {
		let total_width: usize =
			max_widths.iter().sum::<usize>() + (3 * (self.column_count - 1));
		let _ =
			writeln!(out, "{:-<total_width$}", "", total_width = total_width);
	}

	fn print_partial_separator(
		&self,
		out: &mut String,
		max_widths: &[usize],
		data_sep: &[bool],
	) {
		for (i, draw) in data_sep.iter().enumerate() {
			if *draw {
				let _ = write!(out, "{:-<width$}", "", width = max_widths[i]);
			} else {
				let _ = write!(out, "{: <width$}", "", width = max_widths[i]);
			}
			if i < data_sep.len() - 1 {
				let _ = write!(out, "   "); // Spacing between columns
			}
		}
		out.push('\n');
	}

	fn center_align(value: &str, width: usize) -> String {
		if value.len() >= width {
			return value.to_string();
		}
		let total_padding = width - value.len();
		let left_padding = total_padding / 2;
		let right_padding = total_padding - left_padding;

		format!(
			"{}{}{}",
			" ".repeat(left_padding),
			value,
			" ".repeat(right_padding)
		)
	}
}
