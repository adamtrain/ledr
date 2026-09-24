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

//! The command line interface.

use clap::builder::styling::{RgbColor, Style, Styles};
use clap::{Args, Parser, Subcommand};

const EXAMPLES: &str = "\
Examples:
  ledr init                     Start a ledger, with your opening balances
  ledr bs                       Balance sheet (reads $LEDR_FILE)
  ledr -f books.ledr is -P 2024 Income statement for 2024
  ledr is -P last-month -i      Last month, income shown as positive
  ledr bs -c USD                Everything converted to US dollars
  ledr as Checking              Every entry touching a checking account
  ledr add                      Add an entry, with suggestions from history
  ledr add coffee 4.50          Add an entry quickly, like the last coffee
  ledr tidy --write             Align and tidy the ledger files
  ledr check                    Find likely mistakes

Dates: 2024-03-15, 2024-03, 2024, today, yesterday, -30 (days ago).
Periods: any date, 2024-Q1, this-month, last-month, this-quarter,
last-quarter, this-year, last-year, ytd, mtd.

Output is fancy in a terminal and plain when piped. See `man ledr`.";

fn styles() -> Styles {
	let accent = RgbColor(0x7c, 0x83, 0xf7);
	let green = RgbColor(0x1f, 0xbf, 0x8f);
	let faint = RgbColor(0x8a, 0x8a, 0x8a);
	let red = RgbColor(0xf2, 0x50, 0x6e);
	Styles::styled()
		.header(Style::new().bold().fg_color(Some(accent.into())))
		.usage(Style::new().bold().fg_color(Some(accent.into())))
		.literal(Style::new().fg_color(Some(green.into())))
		.placeholder(Style::new().fg_color(Some(faint.into())))
		.error(Style::new().bold().fg_color(Some(red.into())))
		.valid(Style::new().fg_color(Some(green.into())))
		.invalid(Style::new().fg_color(Some(red.into())))
}

#[derive(Parser, Debug)]
#[command(
	name = "ledr",
	version,
	about = "Plain text accounting, with rock-solid math",
	after_help = EXAMPLES,
	styles = styles(),
	max_term_width = 100,
)]
pub struct Cli {
	#[command(flatten)]
	pub global: Global,

	#[command(subcommand)]
	pub command: Option<Command>,
}

#[derive(Args, Debug, Clone)]
pub struct Global {
	/// Ledger file to read, or `-` for stdin (default: the one `ledr init`
	/// set up)
	#[arg(short, long, env = "LEDR_FILE", global = true, value_name = "FILE")]
	pub file: Option<String>,

	/// Ignore entries before this date
	#[arg(short, long, global = true, value_name = "DATE")]
	pub begin: Option<String>,

	/// Ignore entries after this date
	#[arg(short, long, global = true, value_name = "DATE")]
	pub end: Option<String>,

	/// Report on one period, like 2024, 2024-03 or last-month
	#[arg(
		short = 'P',
		long,
		global = true,
		value_name = "PERIOD",
		conflicts_with_all = ["begin", "end"]
	)]
	pub period: Option<String>,

	/// Convert all possible balances to this currency
	#[arg(short, long, global = true, value_name = "CURRENCY")]
	pub currency: Option<String>,

	/// With --currency, ignore balances that cannot be converted
	#[arg(long = "ioc", global = true)]
	pub ignore_other_currencies: bool,

	/// Hide equity accounts
	#[arg(short = 'E', long, global = true)]
	pub ignore_equity: bool,

	/// Condense accounts nested below this depth
	#[arg(short, long, global = true, value_name = "N")]
	pub depth: Option<usize>,

	/// Negate all amounts, e.g. to show income as positive
	#[arg(short, long, global = true)]
	pub invert: bool,

	/// Allow accounts and currencies that were never declared
	#[arg(long, global = true)]
	pub lenient: bool,

	/// Show at most this many decimal places
	#[arg(
		short,
		long,
		global = true,
		value_name = "N",
		value_parser = clap::value_parser!(u32).range(0..=50)
	)]
	pub precision: Option<u32>,

	/// Plain output: no color, panels or charts (the default when piped, or
	/// when LEDR_PLAIN is set)
	#[arg(long, global = true, conflicts_with = "fancy")]
	pub plain: bool,

	/// Fancy output even when not writing to a terminal
	#[arg(long, global = true)]
	pub fancy: bool,
}

#[derive(Subcommand, Debug)]
pub enum Command {
	/// Start a new ledger, with your opening balances, and make it the one
	/// ledr reads
	Init(InitArgs),

	/// Balance sheet: assets, liabilities and equity at a point in time
	#[command(visible_alias = "balance")]
	Bs,

	/// Income statement: income and expenses over a period
	#[command(visible_alias = "income")]
	Is,

	/// Trial balance: every account, to check that the books balance
	#[command(visible_alias = "trial")]
	Tb,

	/// Exchange rates declared, observed or inferred in the ledger
	#[command(visible_alias = "rates")]
	Er,

	/// Realized gains and losses from selling lots
	#[command(visible_alias = "realized")]
	Rgl,

	/// Unrealized gains and losses on lots still held
	#[command(visible_alias = "unrealized")]
	Ugl,

	/// Account summary: every entry touching matching accounts
	#[command(visible_alias = "account")]
	As {
		/// An account name, or any part of one
		account: String,
	},

	/// Print every entry as ledr understands it
	#[command(visible_alias = "print")]
	Fmt,

	/// Print entries whose description matches a search term
	Find {
		/// Text to look for; case-insensitive unless it has capitals
		term: String,
	},

	/// Look for errors and likely mistakes, and report them all
	Check {
		/// Fail on warnings too, e.g. in a pre-commit hook
		#[arg(long)]
		strict: bool,
	},

	/// Add an entry: interactively, or from a quick description
	Add(AddArgs),

	/// Align and tidy ledger files, keeping every comment
	Tidy(TidyArgs),

	/// List every account, with how often and recently it is used
	Accounts,

	/// List every description (payee), with how often it is used
	Payees,

	/// Print a shell completion script
	Completions {
		/// bash, zsh, fish, elvish or powershell
		shell: clap_complete::Shell,
	},
}

#[derive(Args, Debug)]
pub struct AddArgs {
	/// A quick description of the entry, like `coffee 4.50` or
	/// `yesterday rent 1500 @checking`; leave it out to be asked
	pub words: Vec<String>,

	/// The file to add the entry to (default: wherever the latest entry is)
	#[arg(long, value_name = "FILE")]
	pub to: Option<String>,

	/// Don't ask for confirmation before saving
	#[arg(short, long)]
	pub yes: bool,

	/// Show the entry, but don't save it
	#[arg(short = 'n', long)]
	pub dry_run: bool,
}

#[derive(Args, Debug)]
pub struct InitArgs {
	/// Where to create the ledger: a file, or a folder to put main.ledr in
	/// (default: asked, suggesting ~/books/main.ledr)
	pub path: Option<String>,

	/// Don't ask; use the usual accounts, with no balances yet. The main
	/// currency comes from -c, or else your locale.
	#[arg(short, long)]
	pub yes: bool,

	/// Show the ledger, but don't create anything
	#[arg(short = 'n', long)]
	pub dry_run: bool,

	/// Don't make it the ledger ledr reads by default
	#[arg(long)]
	pub no_config: bool,
}

#[derive(Args, Debug)]
pub struct TidyArgs {
	/// Rewrite the files in place (otherwise, show what would change)
	#[arg(short, long, conflicts_with = "check")]
	pub write: bool,

	/// Exit with failure if any file is not tidy, e.g. in CI
	#[arg(long)]
	pub check: bool,
}

#[cfg(test)]
mod tests {
	use super::*;
	use clap::CommandFactory;

	#[test]
	fn test_cli_is_well_formed() {
		Cli::command().debug_assert();
	}

	#[test]
	fn test_flags_work_before_and_after_the_command() {
		let a =
			Cli::try_parse_from(["ledr", "-f", "x", "bs", "-d", "2"]).unwrap();
		let b =
			Cli::try_parse_from(["ledr", "bs", "-f", "x", "-d", "2"]).unwrap();
		assert_eq!(a.global.depth, Some(2));
		assert_eq!(b.global.file.as_deref(), Some("x"));
	}

	#[test]
	fn test_aliases() {
		let cli = Cli::try_parse_from(["ledr", "-f", "x", "balance"]).unwrap();
		assert!(matches!(cli.command, Some(Command::Bs)));
		let cli =
			Cli::try_parse_from(["ledr", "-f", "x", "as", "Assets:A"]).unwrap();
		assert!(matches!(cli.command, Some(Command::As { .. })));
	}

	#[test]
	fn test_init_takes_the_currency_from_the_global_flag() {
		let cli = Cli::try_parse_from(["ledr", "init", "-c", "CAD", "--yes"])
			.unwrap();
		assert_eq!(cli.global.currency.as_deref(), Some("CAD"));
		assert!(matches!(
			cli.command,
			Some(Command::Init(InitArgs { yes: true, .. }))
		));
	}
}
