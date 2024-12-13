/* Copyright © 2024-2025 Adam Train <adam@adamtrain.net>
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
use crate::gl::ledger::VALID_PREFIXES;
use crate::gl::total::Total;
use crate::util::quant::Quant;

/// When using this to display something, you should instantiate it, then sort
/// it, then display it. Filters should be handled in the Total struct.
#[derive(Debug)]
pub struct StatementReporter {
	account: String,
	amounts: Vec<(String, Quant)>, // currency -> balance held
	subtotals: Vec<(String, StatementReporter)>, // account name -> next total
}

impl StatementReporter {
	pub fn from_total(t: Total) -> Self {
		Self {
			account: t.account.clone(),
			amounts: t.amounts().into_iter().collect(),
			subtotals: t
				.subtotals
				.into_iter()
				.map(|(k, v)| (k, StatementReporter::from_total(v)))
				.collect(),
		}
	}

	// -----------
	// -- SORTS --
	// -----------

	/// Sorts top-level by Assets, Liabilities, Equity, Income, Expenses, then
	/// recursively sort in the following way:
	///
	/// Each ordered_total's amounts are first sorted by currency. Each
	/// ordered_total's subtotals beyond the first, which is the special case,
	/// are then sorted in descending order by the absolute value of the sum of
	/// its amounts' Quant components.
	pub fn sort_canonical(&mut self) {
		// Sort amounts by currency
		self.amounts.sort_by(|(a, _), (b, _)| a.cmp(b));

		// Special case: sort the top-level subtotals based on VALID_PREFIXES
		self.subtotals.sort_by_key(|(s, _)| {
			VALID_PREFIXES
				.iter()
				.position(|&prefix| prefix == s)
				.unwrap_or(usize::MAX)
		});

		// Now, sort the rest of the subtotals recursively
		for (_, subtotal) in self.subtotals.iter_mut() {
			subtotal.sort_canonical_recursive();
		}
	}

	fn sort_canonical_recursive(&mut self) {
		// Sort amounts by currency
		self.amounts.sort_by(|(a, _), (b, _)| a.cmp(b));

		// Sort subtotals by the absolute value of the sum of their Quant
		// components,then by sign (positive first), and finally alphabetically
		// by account name
		self.subtotals.sort_by(|(name_a, a), (name_b, b)| {
			let sum_a: Quant =
				a.amounts.iter().map(|(_, money)| *money).sum::<Quant>();
			let sum_b: Quant =
				b.amounts.iter().map(|(_, money)| *money).sum::<Quant>();

			let abs_sum_a = sum_a.abs();
			let abs_sum_b = sum_b.abs();

			match abs_sum_b.cmp(&abs_sum_a) {
				std::cmp::Ordering::Equal => {
					match (sum_b > 0).cmp(&(sum_a > 0)) {
						std::cmp::Ordering::Equal => name_a.cmp(name_b),
						other => other,
					}
				},
				other => other,
			}
		});

		for (_, subtotal) in &mut self.subtotals {
			subtotal.sort_canonical_recursive();
		}
	}

	// ------------
	// -- CHECKS --
	// ------------

	/// Returns true iff this OrderedTotal has the same balance as all subtotals
	/// below it, and there is only one subtotal on each level of depth. In
	/// other words, if true, then it should be intuitive to represent this
	/// balance on a single line, with the account name segments below it all
	/// condensed into one line.
	///
	/// e.g. Instead of
	///     USD -3,000.00    Liabilities
	///       USD -900.00      CreditCards
	///       USD -900.00        Card
	///       USD -400.00          Nested
	///       USD -400.00            SuperFar
	///       USD -400.00              Down
	///
	/// We would get
	///     USD -3,000.00    Liabilities
	///       USD -900.00      CreditCards
	///       USD -900.00        Card
	///       USD -400.00          Nested:SuperFar:Down
	fn can_condense_with_all_below(&self) -> bool {
		if self.subtotals.len() > 1 {
			return false;
		}

		let expected_amounts = &self.amounts;

		for (_, ot) in &self.subtotals {
			// Check if the path is still linear
			if !ot.can_condense_with_all_below() {
				return false;
			}

			if !StatementReporter::amounts_match(expected_amounts, &ot.amounts)
			{
				return false;
			}
		}

		true
	}

	/// Compares two sets of accounts & amounts, and reports true iff they are
	/// all entirely identical.
	fn amounts_match(a: &[(String, Quant)], b: &[(String, Quant)]) -> bool {
		if a.len() != b.len() {
			return false;
		}

		for (currency, amount) in a {
			if let Some((_, other)) = b.iter().find(|(c, _)| c == currency) {
				if amount != other {
					return false;
				}
			} else {
				return false;
			}
		}

		true
	}

	/// Performs the condensation of name discussed and shown above, on the
	/// comment for OrderedTotal::can_condense_with_all_below().
	fn condensed_name(&self) -> String {
		// Check if there are no subtotals; return the current node's name.
		if self.subtotals.is_empty() {
			return self.account.clone();
		}

		let (_, subtotal) = self.subtotals.first().unwrap();

		// Recursively get the name from the next node and concatenate.
		// The if block handles a special case where the top-line can condense.
		if self.account.is_empty() {
			subtotal.condensed_name().to_string()
		} else {
			format!("{}:{}", self.account, subtotal.condensed_name())
		}
	}

	// ------------
	// -- PRINTS --
	// ------------

	pub fn calculate_column_width(&self) -> usize {
		let mut max_width = 0;

		let calculate_width = |currency: &String, amount: &Quant| {
			format!("{} {}", currency, amount).len()
		};

		// Check the width of all amounts in this OrderedTotal
		for (currency, amount) in &self.amounts {
			let width = calculate_width(currency, amount);
			if width > max_width {
				max_width = width;
			}
		}

		// Recursively check all subtotals
		for (_, subtotal) in &self.subtotals {
			let subtotal_width = subtotal.calculate_column_width();
			if subtotal_width > max_width {
				max_width = subtotal_width;
			}
		}

		max_width + 1
	}

	/// Prints the contents of the reporter like the classic Ledger does.
	/// We only expand the subtotals up to the max_depth, if present.
	pub fn print_ledger_format(&self, max_depth: Option<usize>) {
		if self.amounts.is_empty() {
			println!("No data");
			return;
		}

		let column_width = self.calculate_column_width();

		// Display all entries
		self.ledger_fmt_recursive(0, column_width, max_depth);

		// Display the totals for each currency
		println!("{:>width$}", "------------------", width = column_width);
		for (currency, amount) in &self.amounts {
			println!(
				"{:>width$}",
				format!("{} {}", currency, amount),
				width = column_width
			);
		}
	}

	fn ledger_fmt_recursive(
		&self,
		indent: usize,
		width: usize,
		max_depth: Option<usize>,
	) {
		let indentation = " ".repeat(indent * 2);
		let can_condense = self.can_condense_with_all_below();

		// Iterate over amounts and print each one (except top-level)
		if indent != 0 || can_condense {
			let amts = &mut self.amounts.iter().peekable();

			let account_name = if can_condense {
				&self.condensed_name()
			} else {
				&self.account
			};

			let mut has_printed_acct = false;
			while let Some((currency, amount)) = amts.next() {
				// when the same account name would appear on
				// consecutive lines, replace it with a symbol
				let acct = match (has_printed_acct, amts.peek().is_some()) {
					(true, _) => " ↩",
					_ => &*format!(" {}", account_name),
				};

				println!(
					"{:>width$} {}{}",
					format!("{} {}", currency, amount),
					indentation,
					acct,
					width = width
				);

				has_printed_acct = true;
			}
		}

		if let Some(d) = max_depth {
			if indent == d {
				return;
			}
		}

		if !can_condense {
			// Recursively display each subtotal
			for (_, subtotal) in &self.subtotals {
				subtotal.ledger_fmt_recursive(indent + 1, width, max_depth);
			}
		}
	}
}
