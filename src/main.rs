/* Copyright © 2024-2025 Adam Train <adam@trainrelay.net>
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
use crate::gl::total::Total;
use crate::investment::lot::LotStatus;
use crate::investment::portfolio::{LotFilter, Portfolio};
use crate::parsing::filesystem::Filesystem;
use crate::parsing::parser::ParseResult;
use crate::reports::ledger_reporter::LedgerReporter;
use crate::reports::portfolio_reporter::PortfolioReporter;
use crate::reports::rate_reporter::RateReporter;
use crate::reports::statement_reporter::StatementReporter;
use crate::util::date::Date;
use anyhow::{bail, Error};
use chrono::Local;
use clap::{Parser, ValueEnum};
use gl::ledger::Ledger;
use std::cmp::PartialEq;
use std::collections::BTreeMap;

mod config;
mod gl;
mod import;
mod investment;
mod parsing;
mod reports;
mod util;

#[derive(Parser)]
#[command(name = "ledr", version = "1.0", about = "Plain text accounting tool")]
struct Cli {
	// ----------------
	// -- POSITIONAL --
	// ----------------
	/// The command to execute
	command: Directive,

	/// The search term for the AS and Find commands
	#[arg(required = false)]
	term: Option<String>,

	// -----------
	// -- FLAGS --
	// -----------
	/// Specifies the input file
	#[arg(short)]
	file: String,

	/// Ignore entries prior to this date (YYYY-MM-DD)
	#[arg(short, long)]
	begin: Option<String>,

	/// Ignore entries after this date (YYYY-MM-DD)
	#[arg(short, long)]
	end: Option<String>,

	/// Custom config file location (default: ~/.config/ledr/config.toml)
	#[arg(long)]
	config: Option<String>,

	/// Convert all possible balances to this currency
	#[arg(short, long)]
	currency: Option<String>,

	/// Ignore balances that do not resolve to this currency
	#[arg(long = "ioc")]
	ignore_other_currencies: bool,

	/// Hides equity accounts from reports
	#[arg(short = 'E', long)]
	ignore_equity: bool,

	/// Condense accounts nested below this depth
	#[arg(short, long)]
	depth: Option<usize>,

	/// Negates all currency values
	#[arg(short, long)]
	invert: bool,

	/// Ignore directives designed to catch and correct bad input data
	#[arg(long)]
	lenient: bool,

	/// Maximum amount of decimal places to show for any amounts
	#[arg(short, long)]
	precision: Option<u32>,
}

impl Cli {
	/// The point is that this number exceeds what anyone wants; it's just to
	/// stop the program from printing e.g. millions of zeroes by accident
	const MAX_PRECISION: u32 = 50;

	/// Extra validations on top of what clap does
	fn validate(&self) -> Result<(), Error> {
		if let Some(prec) = self.precision {
			if prec > Cli::MAX_PRECISION {
				bail!("Maximum precision is {}", Cli::MAX_PRECISION);
			}
		}

		Ok(())
	}
}

#[derive(ValueEnum, Clone, PartialEq)]
enum Directive {
	Bs, // balance sheet
	Is, // income statement
	Tb, // trial balance

	Er, // exchange rates

	Rgl, // realized gains/losses report
	Ugl, // unrealized gains/losses report

	As,   // account summary
	Fmt,  // format and output the ledger's entries
	Find, // search for entries by description

	Check, // find possible data integrity concerns

	Import, // import data from specific targets
}

fn main() -> Result<(), Error> {
	let args = Cli::parse();
	args.validate()?;

	let (begin, end) = get_range(&args)?;
	let fs = Filesystem::new();

	let mut ledger =
		Ledger::new(args.lenient, args.command == Directive::Check);

	let mut parser = parsing::parser::Parser::new();
	let parse_result = parser.parse(&args.file, &mut ledger, &end)?;

	let portfolio =
		finalize_ledger(&mut ledger, args.precision, &parse_result, &begin)?;

	match args.command {
		Directive::Bs => financial_statement(
			ledger,
			args,
			true,
			vec!["Assets", "Liabilities"],
			parse_result.max_precision_by_currency,
		)?,
		Directive::Is => financial_statement(
			ledger,
			args,
			false,
			vec!["Income", "Expenses"],
			parse_result.max_precision_by_currency,
		)?,
		Directive::Tb => financial_statement(
			ledger,
			args,
			true,
			vec!["Assets", "Liabilities", "Income", "Expenses", "Equity"],
			parse_result.max_precision_by_currency,
		)?,
		Directive::Er => {
			let rates = ledger.exchange_rates.take_all_rates();
			let reporter = RateReporter::new(rates);
			reporter.print_all_rates();
		},
		Directive::Rgl => {
			let reporter = PortfolioReporter::new(
				portfolio.take_lots(vec![LotFilter::HasSales(true)]),
				parse_result.max_precision_by_currency,
				args.precision.unwrap_or(u32::MAX),
			);
			reporter.print_realized_gain_loss(
				&begin,
				&end.min(today()),
				&ledger.exchange_rates,
			)
		},
		Directive::Ugl => {
			let reporter = PortfolioReporter::new(
				portfolio.take_lots(vec![LotFilter::Status(LotStatus::Open)]),
				parse_result.max_precision_by_currency,
				args.precision.unwrap_or(u32::MAX),
			);
			reporter.print_unrealized_gain_loss(
				&end.min(today()),
				&ledger.exchange_rates,
			)
		},
		Directive::As => {
			// Ensure the search term is provided for the AS command
			if let Some(account) = &args.term {
				let entries = LedgerReporter::new(ledger.take_entries());
				// no need to pass ignore_other_currencies in here because
				// this report always does that
				entries.account_summary(account, args.currency)
			} else {
				bail!("No account specified");
			}
		},
		Directive::Find => {
			if args.term.is_none() {
				bail!("No search term specified");
			}
			ledger.print(&begin, args.term);
		},
		Directive::Fmt => {
			ledger.print(&begin, None);
		},
		Directive::Check => {
			// simple log; warnings occur dynamically throughout processing
			println!("Done");
		},
		Directive::Import => {
			// Right now, only this command inspects config in any way, so we
			// don't bother to check for it or parse it until this point
			let config = fs.get_config(args.config.as_ref(), true)?;
			import::importer::import(config, args, ledger)?;
		},
	}

	Ok(())
}

/// Performs validation of the ledger, and returns the portfolio representing
/// the state of lots.
fn finalize_ledger(
	ledger: &mut Ledger,
	max_precision: Option<u32>,
	parse_result: &ParseResult,
	begin: &Date,
) -> Result<Portfolio, Error> {
	ledger
		.exchange_rates
		.finalize(&parse_result.max_precision_by_currency)?;

	let portfolio = ledger.lots.tabulate()?;

	ledger.finalize(
		begin,
		&parse_result.max_precision_by_currency,
		max_precision,
	)?;

	Ok(portfolio)
}

fn financial_statement(
	ledger: Ledger,
	args: Cli,
	include_equity_by_default: bool,
	top_level_accounts_to_show: Vec<&str>,
	mut max_precision_by_currency: BTreeMap<String, u32>,
) -> Result<(), Error> {
	let mut totals = ledger_to_totals(
		ledger,
		args.currency,
		args.invert,
		args.ignore_other_currencies,
	)?;

	totals.round(
		args.precision.unwrap_or(u32::MAX),
		&mut max_precision_by_currency,
		true,
	);

	let mut top_levels = top_level_accounts_to_show;
	if include_equity_by_default && !args.ignore_equity {
		top_levels.push("Equity");
	}
	totals.filter_top_level(top_levels);
	let mut reporter = StatementReporter::from_total(totals);

	reporter.sort_canonical();
	reporter.print_ledger_format(args.depth);
	Ok(())
}

fn ledger_to_totals(
	mut ledger: Ledger,
	collapse: Option<String>,
	invert: bool,
	ignore_other_currencies: bool,
) -> Result<Total, Error> {
	let mut totals = Total::from_ledger(&ledger);

	if let Some(collapse) = &collapse {
		totals.collapse_to(
			collapse,
			&mut ledger.exchange_rates,
			ignore_other_currencies,
		);
	}

	if invert {
		totals.invert();
	}

	Ok(totals)
}

fn get_range(args: &Cli) -> Result<(Date, Date), Error> {
	let mut begin = Date::from_str(
		args.begin.as_ref().unwrap_or(&Date::min().to_string()),
	)?;
	let end =
		Date::from_str(args.end.as_ref().unwrap_or(&Date::max().to_string()))?;

	// for importing, we want to use the entire ledger up to that point for
	// account matching
	if args.command == Directive::Import {
		begin = Date::min();
	}

	Ok((begin, end))
}

fn today() -> Date {
	Date::from_str(&Local::now().date_naive().to_string()).unwrap()
}
