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

//! End-to-end tests of the command line: adding, tidying, errors, and the
//! fancy layouts (compared, without color, against snapshots).
//!
//! To update the snapshots after an intended change to a layout, run
//! `LEDR_UPDATE_SNAPSHOTS=1 cargo test --test cli` and review the diff.

use std::fs;
use std::path::Path;
use std::process::{Command, Output};

const LEDGER: &str = "\
! 2024-11-01 currency USD
! 2024-11-01 currency CAD
! 2024-11-01 account Assets:Checking
! 2024-11-01 account Liabilities:Visa
! 2024-11-01 account Income:Salary
! 2024-11-01 account Expenses:Food:Coffee
! 2024-11-01 account Expenses:Food:Groceries
! 2024-11-01 account Expenses:Rent
! 2024-11-01 account Equity:Opening

2024-11-01 Opening balance
    Assets:Checking   1,000.00 USD
    Equity:Opening

2024-11-01 Rent
    Expenses:Rent   800.00 USD
    Assets:Checking

2024-11-02 Tim Hortons
    Expenses:Food:Coffee   4.50 USD
    Liabilities:Visa

2024-11-09 Trader Joe's
    Expenses:Food:Groceries   62.10 USD
    Liabilities:Visa

2024-11-15 Paycheck
    Assets:Checking   2,500.00 USD
    Income:Salary
";

fn ledr(args: &[&str], env: &[(&str, &str)]) -> Output {
	ledr_in(None, args, env)
}

/// Runs ledr, in `dir` if given, with `env` on top of a predictable
/// environment
fn ledr_in(dir: Option<&Path>, args: &[&str], env: &[(&str, &str)]) -> Output {
	let mut command = Command::new(env!("CARGO_BIN_EXE_ledr"));
	command
		.args(args)
		.env("NO_COLOR", "1")
		.env("COLUMNS", "80")
		.env_remove("LEDR_FILE")
		.env_remove("LEDR_PLAIN")
		// Never the config or locale of whoever runs the tests
		.env("HOME", "/nonexistent/ledr-tests/home")
		.env("XDG_CONFIG_HOME", "/nonexistent/ledr-tests/config")
		.env_remove("LC_ALL")
		.env_remove("LC_MONETARY")
		.env("LANG", "en_US.UTF-8");
	if let Some(dir) = dir {
		command.current_dir(dir);
	}
	for (key, value) in env {
		command.env(key, value);
	}
	command.output().expect("ledr runs")
}

fn stdout(output: &Output) -> String {
	String::from_utf8_lossy(&output.stdout).into_owned()
}

fn stderr(output: &Output) -> String {
	String::from_utf8_lossy(&output.stderr).into_owned()
}

/// A ledger in a fresh temporary directory
fn ledger(text: &str) -> (tempfile::TempDir, String) {
	let dir = tempfile::tempdir().unwrap();
	let path = dir.path().join("books.ledr");
	fs::write(&path, text).unwrap();
	let path = path.to_str().unwrap().to_string();
	(dir, path)
}

fn snapshot(name: &str, actual: &str) {
	let path = Path::new("tests/snapshots").join(format!("{name}.txt"));
	if std::env::var_os("LEDR_UPDATE_SNAPSHOTS").is_some() {
		fs::write(&path, actual).unwrap();
		return;
	}
	let expected = fs::read_to_string(&path)
		.unwrap_or_else(|_| panic!("no snapshot at {}", path.display()));
	assert_eq!(actual, expected, "{name} no longer matches its snapshot");
}

#[test]
fn test_fancy_layouts() {
	let (_dir, file) = ledger(LEDGER);
	let cases: [(&str, &[&str]); 5] = [
		("balance_sheet", &["bs"]),
		("income_statement", &["is", "-i"]),
		("trial_balance", &["tb"]),
		("account", &["as", "Checking"]),
		("find", &["find", "tim"]),
	];
	for (name, args) in cases {
		let mut all = vec!["-f", &file, "--fancy"];
		all.extend_from_slice(args);
		let output = ledr(&all, &[]);
		assert!(output.status.success(), "{name}: {}", stderr(&output));
		snapshot(name, &stdout(&output));
	}
}

#[test]
fn test_output_is_plain_when_piped() {
	let (_dir, file) = ledger(LEDGER);
	let piped = ledr(&["-f", &file, "bs"], &[]);
	let plain = ledr(&["-f", &file, "bs", "--plain"], &[]);
	assert_eq!(stdout(&piped), stdout(&plain));
	assert!(!stdout(&piped).contains('╭'));
}

#[test]
fn test_ledger_file_from_environment() {
	let (_dir, file) = ledger(LEDGER);
	let output = ledr(&["tb"], &[("LEDR_FILE", &file)]);
	assert!(output.status.success(), "{}", stderr(&output));
	let missing = ledr(&["tb"], &[]);
	assert!(!missing.status.success());
	assert!(stderr(&missing).contains("LEDR_FILE"));
}

#[test]
fn test_periods_match_explicit_dates() {
	let (_dir, file) = ledger(LEDGER);
	let period = ledr(&["-f", &file, "is", "-P", "2024-11"], &[]);
	let dates = ledr(
		&["-f", &file, "is", "-b", "2024-11-01", "-e", "2024-11-30"],
		&[],
	);
	let month =
		ledr(&["-f", &file, "is", "-b", "2024-11", "-e", "2024-11"], &[]);
	assert_eq!(stdout(&period), stdout(&dates));
	assert_eq!(stdout(&period), stdout(&month));
	assert!(stdout(&period).contains("Salary"));
}

#[test]
fn test_quick_add_appends_a_checked_entry() {
	let (_dir, file) = ledger(LEDGER);
	let output = ledr(
		&["-f", &file, "add", "2024-11-20", "tim", "5.25", "--yes"],
		&[],
	);
	assert!(output.status.success(), "{}", stderr(&output));
	assert!(stdout(&output).contains("Saved to"));

	let text = fs::read_to_string(&file).unwrap();
	assert!(
		text.ends_with("\n\n2024-11-20 Tim Hortons\n    Expenses:Food:Coffee         5.25 USD\n    Liabilities:Visa\n"),
		"{text}"
	);
	let check = ledr(&["-f", &file, "check"], &[]);
	assert!(check.status.success(), "{}", stdout(&check));
}

#[test]
fn test_add_declares_new_accounts() {
	let (_dir, file) = ledger(LEDGER);
	let output = ledr(
		&[
			"-f",
			&file,
			"add",
			"2024-11-21",
			"New bike",
			"300",
			"@Expenses:Hobbies",
			"@visa",
			"--yes",
		],
		&[],
	);
	assert!(output.status.success(), "{}", stderr(&output));
	let text = fs::read_to_string(&file).unwrap();
	assert!(text.contains(
		"! 2024-11-21 account Expenses:Hobbies\n2024-11-21 New bike\n"
	));
	assert!(ledr(&["-f", &file, "check"], &[]).status.success());
}

#[test]
fn test_add_writes_nothing_it_cannot_check() {
	let (_dir, file) = ledger(LEDGER);
	// The card is closed later in the month, so entries after that fail
	let text = format!("{LEDGER}\n! 2024-11-25 close Liabilities:Visa\n");
	fs::write(&file, &text).unwrap();
	let broken = ledr(
		&["-f", &file, "add", "2024-11-30", "tim", "3", "--yes"],
		&[],
	);
	assert!(!broken.status.success());
	assert!(
		stderr(&broken).contains("Liabilities:Visa is not open"),
		"{}",
		stderr(&broken)
	);
	assert_eq!(
		fs::read_to_string(&file).unwrap(),
		text,
		"the file must be untouched"
	);

	let dry = ledr(
		&["-f", &file, "add", "2024-11-20", "tim", "3", "--dry-run"],
		&[],
	);
	assert!(dry.status.success(), "{}", stderr(&dry));
	assert_eq!(fs::read_to_string(&file).unwrap(), text);

	let unconfirmed =
		ledr(&["-f", &file, "add", "2024-11-20", "tim", "3"], &[]);
	assert!(!unconfirmed.status.success(), "no terminal to confirm with");
	assert_eq!(fs::read_to_string(&file).unwrap(), text);
}

#[test]
fn test_tidy_changes_layout_but_not_meaning() {
	let messy = "\
! 2024-11-01   currency USD
2024-11-1 Coffee  # morning
  Expenses:Coffee 4.5 USD
  Assets:Cash    -4.5  USD



2024-11-02 Rent
  Expenses:Rent   1,200.00 USD
  Assets:Cash
";
	let (_dir, file) = ledger(messy);
	let before = stdout(&ledr(&["-f", &file, "--lenient", "tb"], &[]));

	let check = ledr(&["-f", &file, "tidy", "--check"], &[]);
	assert!(!check.status.success());

	let diff = ledr(&["-f", &file, "tidy"], &[]);
	assert!(diff.status.success());
	assert!(stdout(&diff).contains("-2024-11-1 Coffee"));
	assert_eq!(
		fs::read_to_string(&file).unwrap(),
		messy,
		"showing a diff writes nothing"
	);

	assert!(
		ledr(&["-f", &file, "tidy", "--write"], &[])
			.status
			.success()
	);
	assert_eq!(
		fs::read_to_string(&file).unwrap(),
		"\
! 2024-11-01 currency USD
2024-11-01 Coffee  # morning
  Expenses:Coffee      4.5 USD
  Assets:Cash         -4.5 USD


2024-11-02 Rent
  Expenses:Rent    1,200.00 USD
  Assets:Cash
"
	);
	assert!(
		ledr(&["-f", &file, "tidy", "--check"], &[])
			.status
			.success()
	);
	assert_eq!(
		stdout(&ledr(&["-f", &file, "--lenient", "tb"], &[])),
		before
	);
}

#[test]
fn test_errors_point_at_the_line() {
	let (_dir, file) =
		ledger("2024-01-01 Lunch\n    Expenses:Food  12.00\n    Assets:Cash\n");
	let output = ledr(&["-f", &file, "--lenient", "bs"], &[]);
	assert!(!output.status.success());
	let err = stderr(&output);
	assert!(
		err.contains("books.ledr:2: Missing a currency after `12.00`"),
		"{err}"
	);
	assert!(
		err.contains("help: write the currency after the amount"),
		"{err}"
	);
}

#[test]
fn test_check_reports_everything_at_once() {
	let text = format!(
		"{LEDGER}\n2024-11-20 Typo\n    Expenses:Food:Cofee  1.00 USD\n    Assets:Checking\n\n2024-11-21 Unbalanced\n    Expenses:Rent  5.00 USD\n    Assets:Checking  -4.00 USD\n"
	);
	let (_dir, file) = ledger(&text);
	let output = ledr(&["-f", &file, "check"], &[]);
	assert!(!output.status.success());
	let out = stdout(&output);
	assert!(out.contains("did you mean Expenses:Food:Coffee?"), "{out}");
	assert!(out.contains("off by 1.00 USD"), "{out}");
	assert!(out.contains("2 errors"), "{out}");
}

#[test]
fn test_strict_check_fails_on_warnings() {
	let (_dir, file) = ledger(
		"! 2024-01-01 rate CAD USD 5\n\n2024-01-01 Swap\n    Assets:A  1 CAD\n    Assets:B  -1 USD\n",
	);
	let relaxed = ledr(&["-f", &file, "--lenient", "check"], &[]);
	assert!(relaxed.status.success());
	assert!(stdout(&relaxed).contains("1 warning"));
	let strict = ledr(&["-f", &file, "--lenient", "check", "--strict"], &[]);
	assert!(!strict.status.success());
}

#[test]
fn test_completions() {
	let output = ledr(&["completions", "zsh"], &[]);
	assert!(output.status.success());
	assert!(stdout(&output).contains("ledr"));
}

#[test]
fn test_overview() {
	let (_dir, file) = ledger(LEDGER);
	let output = ledr(&["-f", &file, "--fancy"], &[]);
	assert!(output.status.success(), "{}", stderr(&output));
	let out = stdout(&output);
	assert!(out.contains("Net worth"), "{out}");
	assert!(out.contains("Spending in November 2024"), "{out}");
}

#[test]
fn test_total_price_on_a_negative_amount() {
	// Paying 10 USD with a total of 13 CAD takes 13 CAD, not adds it
	let (_dir, file) = ledger(
		"2024-01-01 Exchange\n    Assets:USD  -10 USD @@ 13 CAD\n    Assets:CAD\n",
	);
	let output =
		stdout(&ledr(&["-f", &file, "--lenient", "bs", "--plain"], &[]));
	assert!(
		output.contains("CAD 13    ↩") || output.contains("CAD 13"),
		"{output}"
	);
	assert!(!output.contains("CAD -13      CAD"), "{output}");
	let rates =
		stdout(&ledr(&["-f", &file, "--lenient", "er", "--plain"], &[]));
	assert!(!rates.contains("-1"), "rates must not be negative: {rates}");
}

#[test]
fn test_gains_never_use_rates_from_after_the_sale_or_from_the_lot_itself() {
	let text = "\
2024-01-10 Buy
    Assets:Broker   10 AAPL { 100.00 USD }
    Assets:Cash    -1000.00 USD

2024-03-10 Sell for CAD
    Assets:Broker  -10 AAPL { 100.00 USD }
    Assets:Cad      1875.00 CAD
";
	let (_dir, file) = ledger(text);
	let unknown =
		stdout(&ledr(&["-f", &file, "--lenient", "rgl", "--plain"], &[]));
	assert!(
		unknown.contains("UNK"),
		"no USD/CAD rate existed: {unknown}"
	);

	let later = format!("{text}\n! 2024-04-01 rate CAD USD 0.75\n");
	fs::write(&file, later).unwrap();
	let still =
		stdout(&ledr(&["-f", &file, "--lenient", "rgl", "--plain"], &[]));
	assert!(
		still.contains("UNK"),
		"that rate came after the sale: {still}"
	);

	let before = format!("{text}\n! 2024-03-01 rate CAD USD 0.75\n");
	fs::write(&file, before).unwrap();
	let known =
		stdout(&ledr(&["-f", &file, "--lenient", "rgl", "--plain"], &[]));
	assert!(
		known.contains("406.25 USD"),
		"187.5 CAD at 0.75 is 140.625 USD a share: {known}"
	);
}

#[test]
fn test_add_checks_the_whole_ledger_whatever_the_range() {
	let (_dir, file) =
		ledger(&format!("{LEDGER}\n! 2024-11-25 close Liabilities:Visa\n"));
	let original = fs::read_to_string(&file).unwrap();
	for range in [["-e", "2024-11-10"], ["-P", "2024-10"]] {
		let output = ledr(
			&[
				"-f",
				&file,
				range[0],
				range[1],
				"add",
				"2024-11-30",
				"tim",
				"3",
				"--yes",
			],
			&[],
		);
		assert!(
			!output.status.success(),
			"{range:?} must not skip the check"
		);
		assert_eq!(fs::read_to_string(&file).unwrap(), original);
	}
}

#[test]
fn test_add_refuses_files_outside_the_ledger_and_stdin() {
	let (dir, file) = ledger(LEDGER);
	let notes = dir.path().join("notes.txt");
	fs::write(&notes, "").unwrap();
	let notes = notes.to_str().unwrap();
	let output = ledr(
		&["-f", &file, "add", "tim", "3", "--yes", "--to", notes],
		&[],
	);
	assert!(!output.status.success());
	assert!(
		stderr(&output).contains("isn't part of this ledger"),
		"{}",
		stderr(&output)
	);
	assert_eq!(fs::read_to_string(notes).unwrap(), "");

	let stdin = ledr(&["-f", "-", "add", "tim", "3", "--yes"], &[]);
	assert!(!stdin.status.success());
	assert!(stderr(&stdin).contains("stdin"));
}

#[test]
fn test_add_refuses_text_that_would_read_back_differently() {
	let (_dir, file) = ledger(LEDGER);
	let output = ledr(
		&[
			"-f",
			&file,
			"add",
			"Order #1234",
			"20",
			"@groceries",
			"@visa",
			"--yes",
		],
		&[],
	);
	assert!(!output.status.success());
	assert!(
		stderr(&output).contains("would start a comment"),
		"{}",
		stderr(&output)
	);
	assert_eq!(fs::read_to_string(&file).unwrap(), LEDGER);
}

#[test]
fn test_add_keeps_windows_line_endings() {
	let (_dir, file) = ledger(&LEDGER.replace('\n', "\r\n"));
	let output = ledr(
		&["-f", &file, "add", "2024-11-20", "tim", "5", "--yes"],
		&[],
	);
	assert!(output.status.success(), "{}", stderr(&output));
	let text = fs::read_to_string(&file).unwrap();
	assert!(
		!text.replace("\r\n", "").contains('\n'),
		"only CRLF line endings"
	);
	assert!(text.ends_with("\r\n\r\n2024-11-20 Tim Hortons\r\n    Expenses:Food:Coffee         5.00 USD\r\n    Liabilities:Visa\r\n"), "{text:?}");
}

#[cfg(unix)]
#[test]
fn test_tidy_writes_through_symlinks_and_spares_read_only_files() {
	use std::os::unix::fs::{PermissionsExt, symlink};

	let dir = tempfile::tempdir().unwrap();
	let real = dir.path().join("real.ledr");
	let link = dir.path().join("link.ledr");
	fs::write(&real, "2024-1-2 X\n  Assets:A  1 USD\n  Assets:B\n").unwrap();
	symlink(&real, &link).unwrap();

	let output = ledr(&["-f", link.to_str().unwrap(), "tidy", "--write"], &[]);
	assert!(output.status.success(), "{}", stderr(&output));
	assert!(
		fs::symlink_metadata(&link)
			.unwrap()
			.file_type()
			.is_symlink()
	);
	assert!(
		fs::read_to_string(&real)
			.unwrap()
			.starts_with("2024-01-02 X")
	);

	fs::write(&real, "2024-1-3 Y\n  Assets:A  1 USD\n  Assets:B\n").unwrap();
	fs::set_permissions(&real, fs::Permissions::from_mode(0o444)).unwrap();
	let output = ledr(&["-f", real.to_str().unwrap(), "tidy", "--write"], &[]);
	assert!(!output.status.success());
	assert!(stderr(&output).contains("read-only"), "{}", stderr(&output));
	assert!(fs::read_to_string(&real).unwrap().starts_with("2024-1-3 Y"));
	let leftovers: Vec<_> = fs::read_dir(dir.path())
		.unwrap()
		.filter(|e| {
			e.as_ref()
				.unwrap()
				.file_name()
				.to_string_lossy()
				.contains("ledr-tidy")
		})
		.collect();
	assert!(leftovers.is_empty());
}

#[test]
fn test_balance_sheet_counts_everything_before_its_end() {
	let (_dir, file) = ledger(LEDGER);
	let all = stdout(&ledr(&["-f", &file, "bs", "-e", "2024-11-30"], &[]));
	let late_start = stdout(&ledr(
		&["-f", &file, "bs", "-b", "2024-11-10", "-e", "2024-11-30"],
		&[],
	));
	assert_eq!(all, late_start, "-b does not apply to a balance sheet");
	assert!(all.contains("USD 2,700.00"), "{all}");
}

#[test]
fn test_account_balance_starts_from_earlier_entries() {
	let (_dir, file) = ledger(LEDGER);
	let plain = stdout(&ledr(
		&["-f", &file, "as", "Checking", "-b", "2024-11-10"],
		&[],
	));
	assert!(plain.contains("Paycheck"), "{plain}");
	assert!(!plain.contains("Opening balance"), "{plain}");
	let fancy = stdout(&ledr(
		&["-f", &file, "--fancy", "as", "Checking", "-b", "2024-11-10"],
		&[],
	));
	assert!(
		fancy.contains("Balance     2,700.00 USD"),
		"includes the 200.00 before: {fancy}"
	);
}

/// A home directory of its own, with nothing in it yet
struct Home {
	dir: tempfile::TempDir,
}

impl Home {
	fn new() -> Self {
		Self {
			dir: tempfile::tempdir().unwrap(),
		}
	}

	fn path(&self, relative: &str) -> std::path::PathBuf {
		self.dir.path().join(relative)
	}

	/// Runs ledr from the home directory, with its config under it
	fn run(&self, args: &[&str], env: &[(&str, &str)]) -> Output {
		let home = self.dir.path().to_str().unwrap();
		let mut all = vec![("HOME", home), ("XDG_CONFIG_HOME", "")];
		all.extend_from_slice(env);
		ledr_in(Some(self.dir.path()), args, &all)
	}
}

#[test]
fn test_init_creates_a_ledger_that_ledr_then_reads() {
	let home = Home::new();
	let output = home.run(&["init", "--yes", "-c", "EUR"], &[]);
	assert!(output.status.success(), "{}", stderr(&output));
	assert!(stdout(&output).contains("Created ~/books/main.ledr"));

	let text = fs::read_to_string(home.path("books/main.ledr")).unwrap();
	assert!(text.contains("currency EUR"));
	assert!(text.contains("account Assets:Checking"));
	assert!(text.contains("account Equity:OpeningBalances"));
	let config =
		fs::read_to_string(home.path(".config/ledr/config.toml")).unwrap();
	assert!(config.contains("file = '~/books/main.ledr'"), "{config}");

	// No -f needed from now on, and what init wrote is a sound ledger
	let check = home.run(&["check", "--strict"], &[]);
	assert!(check.status.success(), "{}", stderr(&check));
	let add =
		home.run(&["add", "groceries", "54.20", "@checking", "--yes"], &[]);
	assert!(add.status.success(), "{}", stderr(&add));
	let bs = home.run(&["bs"], &[]);
	assert!(stdout(&bs).contains("EUR -54.20"), "{}", stdout(&bs));
}

#[test]
fn test_init_follows_the_locale() {
	let home = Home::new();
	let output = home.run(&["init", "--yes"], &[("LANG", "en_CA.UTF-8")]);
	assert!(output.status.success(), "{}", stderr(&output));
	let text = fs::read_to_string(home.path("books/main.ledr")).unwrap();
	assert!(text.contains("currency CAD"));
	assert!(text.contains("account Assets:Chequing"));
	// The example it suggests works on the new ledger
	assert!(stdout(&output).contains("ledr add groceries 54.20 @creditcard"));
	let add = home.run(
		&["add", "groceries", "54.20", "@creditcard", "--dry-run"],
		&[],
	);
	assert!(add.status.success(), "{}", stderr(&add));
}

#[test]
fn test_init_dry_run_writes_nothing() {
	let home = Home::new();
	let output = home.run(&["init", "--dry-run", "books.ledr"], &[]);
	assert!(output.status.success(), "{}", stderr(&output));
	assert!(stdout(&output).contains("! "));
	assert!(stdout(&output).contains("Dry run"));
	assert!(!home.path("books.ledr").exists());
	assert!(!home.path(".config").exists());
}

#[test]
fn test_init_asks_for_a_terminal_or_yes() {
	let home = Home::new();
	let output = home.run(&["init"], &[]);
	assert!(!output.status.success());
	assert!(stderr(&output).contains("--yes"), "{}", stderr(&output));
	assert!(!home.path("books").exists());
}

#[test]
fn test_init_never_touches_an_existing_ledger() {
	let home = Home::new();
	fs::write(home.path("mine.ledr"), LEDGER).unwrap();
	let output = home.run(&["init", "--yes", "mine.ledr"], &[]);
	assert!(output.status.success(), "{}", stderr(&output));
	assert!(stdout(&output).contains("already exists"));
	assert!(stdout(&output).contains("a ledger of 5 entries"));
	assert_eq!(fs::read_to_string(home.path("mine.ledr")).unwrap(), LEDGER);
	// It became the default
	let tb = home.run(&["tb"], &[]);
	assert!(tb.status.success(), "{}", stderr(&tb));

	// Something that isn't a ledger is refused, and left alone
	fs::write(home.path("notes.txt"), "hello there\n").unwrap();
	let output = home.run(&["init", "--yes", "notes.txt"], &[]);
	assert!(!output.status.success());
	assert!(stderr(&output).contains("couldn't read it as a ledger"));
}

#[test]
fn test_init_a_folder_and_without_config() {
	let home = Home::new();
	fs::create_dir(home.path("money")).unwrap();
	let output = home.run(&["init", "--yes", "--no-config", "money"], &[]);
	assert!(output.status.success(), "{}", stderr(&output));
	assert!(home.path("money/main.ledr").exists());
	assert!(!home.path(".config").exists());
	assert!(stdout(&output).contains("ledr -f ~/money/main.ledr add"));
}

#[test]
fn test_config_problems_are_explained() {
	let home = Home::new();
	fs::create_dir_all(home.path(".config/ledr")).unwrap();
	let config = home.path(".config/ledr/config.toml");

	fs::write(&config, "# mine\nfiel = 'x.ledr'\n").unwrap();
	let output = home.run(&["bs"], &[]);
	assert!(!output.status.success());
	let err = stderr(&output);
	assert!(err.contains("config.toml:2:"), "{err}");
	assert!(err.contains("unknown field `fiel`"), "{err}");

	fs::write(&config, "file = '~/gone.ledr'\n").unwrap();
	let output = home.run(&["bs"], &[]);
	assert!(!output.status.success());
	assert!(stderr(&output).contains("`~/gone.ledr`, doesn't exist"));
	assert!(stderr(&output).contains("ledr init"));

	// -f and LEDR_FILE come before the config
	let (_dir, file) = ledger(LEDGER);
	let output = home.run(&["tb"], &[("LEDR_FILE", &file)]);
	assert!(output.status.success(), "{}", stderr(&output));
}

#[test]
fn test_init_warns_when_ledr_file_would_win() {
	let home = Home::new();
	let (_dir, file) = ledger(LEDGER);
	let output =
		home.run(&["init", "--yes", "new.ledr"], &[("LEDR_FILE", &file)]);
	assert!(output.status.success(), "{}", stderr(&output));
	assert!(stdout(&output).contains("LEDR_FILE is set"));
}

#[test]
fn test_welcome_without_a_ledger() {
	let home = Home::new();
	let fancy = home.run(&["--fancy"], &[]);
	assert!(fancy.status.success());
	assert!(stdout(&fancy).contains("ledr init"));
	assert!(stdout(&fancy).contains('╭'));
	// Plain output stays the usual help
	let plain = home.run(&[], &[]);
	assert!(stdout(&plain).contains("Usage:"));
}
