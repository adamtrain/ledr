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

//! Starting a new ledger: what someone has, what they owe and what they
//! spend on, written out as declarations and an opening balances entry.

use crate::config;
use crate::gl::ledger::VALID_PREFIXES;
use crate::input::compose::{Draft, DraftPosting};
use crate::input::expr::Typed;
use crate::input::history::History;
use crate::tidy::FileStyle;
use crate::util::amount::DEFAULT_PRECISION;
use crate::util::date::Date;
use crate::util::quant::Quant;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// Where opening balances come from
pub const OPENING: &str = "Equity:OpeningBalances";

/// Where a new ledger goes unless someone says otherwise
pub const DEFAULT_PATH: &str = "~/books/main.ledr";

/// A new ledger, as someone described it
#[derive(Clone, Debug, PartialEq)]
pub struct Setup {
	/// When the books start, which is the date of the opening balances
	pub date: Date,
	/// The currency most things are in
	pub currency: String,
	/// Accounts that hold or owe money, in the order they were given
	pub accounts: Vec<Opening>,
	/// Income and expense accounts to start with
	pub categories: Vec<String>,
}

/// An account and what it held when the books start
#[derive(Clone, Debug, PartialEq)]
pub struct Opening {
	pub account: String,
	/// As it would be posted, so money owed is negative. None if it's
	/// empty, or not known yet.
	pub balance: Option<(Quant, String)>,
}

impl Opening {
	pub fn new(
		account: impl Into<String>,
		balance: Option<(Quant, &str)>,
	) -> Self {
		Self {
			account: account.into(),
			balance: balance.map(|(v, c)| (v, c.to_string())),
		}
	}
}

/// Expense categories to offer, and whether each starts out chosen
pub const EXPENSES: &[(&str, bool)] = &[
	("Housing:Rent", true),
	("Housing:MortgageInterest", false),
	("Housing:Utilities", true),
	("Housing:Internet", true),
	("Housing:Maintenance", false),
	("Food:Groceries", true),
	("Food:Restaurants", true),
	("Food:Coffee", false),
	("Transport", true),
	("Health", true),
	("Insurance", false),
	("Phone", true),
	("Subscriptions", true),
	("Shopping", true),
	("Entertainment", true),
	("Travel", true),
	("Gifts", true),
	("Education", false),
	("Childcare", false),
	("Pets", false),
	("Taxes", false),
	("Fees", true),
	("Interest", true),
];

/// Income categories to offer, and whether each starts out chosen
pub const INCOME: &[(&str, bool)] = &[
	("Salary", true),
	("Interest", true),
	("Gifts", false),
	("Business", false),
	("Dividends", false),
	("Refunds", false),
];

/// Names to suggest for money owed
pub const DEBTS: &[&str] = &[
	"CreditCard",
	"Visa",
	"Mastercard",
	"Amex",
	"StudentLoan",
	"CarLoan",
	"Mortgage",
	"LineOfCredit",
	"PersonalLoan",
];

/// Names to suggest for accounts that hold money, as they're usually
/// called where someone lives
pub fn holdings(region: Option<&str>) -> &'static [&'static str] {
	match region {
		Some("CA") => {
			&["Chequing", "Savings", "Cash", "TFSA", "RRSP", "Brokerage"]
		},
		Some("US") => {
			&["Checking", "Savings", "Cash", "Brokerage", "IRA", "401k"]
		},
		Some("GB") => &["Current", "Savings", "Cash", "ISA", "Pension"],
		Some("IE") => &["Current", "Savings", "Cash", "Pension"],
		Some("AU") => &["Everyday", "Savings", "Cash", "Super"],
		Some("NZ") => &["Everyday", "Savings", "Cash", "KiwiSaver"],
		_ => &["Checking", "Savings", "Cash", "Brokerage", "Retirement"],
	}
}

/// Each currency, with the regions that use it, roughly by how many people
/// keep books in it
const CURRENCIES: &[(&str, &[&str])] = &[
	("USD", &["US", "PR", "EC", "SV", "PA"]),
	(
		"EUR",
		&[
			"AT", "BE", "BG", "CY", "DE", "EE", "ES", "FI", "FR", "GR", "HR",
			"IE", "IT", "LT", "LU", "LV", "MT", "NL", "PT", "SI", "SK",
		],
	),
	("GBP", &["GB"]),
	("CAD", &["CA"]),
	("AUD", &["AU"]),
	("JPY", &["JP"]),
	("CHF", &["CH", "LI"]),
	("CNY", &["CN"]),
	("INR", &["IN"]),
	("NZD", &["NZ"]),
	("SEK", &["SE"]),
	("NOK", &["NO"]),
	("DKK", &["DK"]),
	("PLN", &["PL"]),
	("CZK", &["CZ"]),
	("HUF", &["HU"]),
	("RON", &["RO"]),
	("ISK", &["IS"]),
	("MXN", &["MX"]),
	("BRL", &["BR"]),
	("ARS", &["AR"]),
	("CLP", &["CL"]),
	("COP", &["CO"]),
	("ZAR", &["ZA"]),
	("SGD", &["SG"]),
	("HKD", &["HK"]),
	("KRW", &["KR"]),
	("TWD", &["TW"]),
	("THB", &["TH"]),
	("MYR", &["MY"]),
	("IDR", &["ID"]),
	("PHP", &["PH"]),
	("VND", &["VN"]),
	("ILS", &["IL"]),
	("TRY", &["TR"]),
	("AED", &["AE"]),
	("SAR", &["SA"]),
	("EGP", &["EG"]),
	("NGN", &["NG"]),
	("KES", &["KE"]),
	("PKR", &["PK"]),
	("UAH", &["UA"]),
];

/// Every currency code ledr knows a region for, to suggest
pub fn currency_codes() -> impl Iterator<Item = &'static str> {
	CURRENCIES.iter().map(|(code, _)| *code)
}

/// Currencies usually written without decimal places
const WHOLE_CURRENCIES: &[&str] = &["JPY", "KRW", "VND", "CLP", "ISK"];

/// The decimal places a currency is usually written with
pub fn decimals(currency: &str) -> u32 {
	if WHOLE_CURRENCIES.contains(&currency) {
		0
	} else {
		DEFAULT_PRECISION
	}
}

/// Symbols that more than one currency writes itself with
const SHARED_SYMBOLS: &[(char, &[&str])] = &[
	(
		'$',
		&[
			"USD", "CAD", "AUD", "NZD", "SGD", "HKD", "TWD", "MXN", "ARS",
			"CLP", "COP",
		],
	),
	('¥', &["JPY", "CNY"]),
];

/// The currency of an amount typed while setting up: the one it names, or
/// else the main one. A `$` means the main currency if that is a kind of
/// dollar, since to a Canadian, `$20` is twenty Canadian dollars.
pub fn currency_of(input: &str, typed: &Typed, main: &str) -> String {
	let shared = SHARED_SYMBOLS.iter().any(|(symbol, codes)| {
		input.contains(*symbol) && codes.contains(&main)
	});
	match &typed.currency {
		Some(_) if shared && !input.chars().any(char::is_alphabetic) => {
			main.to_string()
		},
		Some(currency) => currency.to_uppercase(),
		None => main.to_string(),
	}
}

/// The region of a locale like `en_CA.UTF-8`
pub fn region_of(locale: &str) -> Option<String> {
	let base = locale.split(['.', '@']).next()?;
	let (_, region) = base.split_once(['_', '-'])?;
	(region.len() == 2 && region.chars().all(|c| c.is_ascii_alphabetic()))
		.then(|| region.to_ascii_uppercase())
}

/// The locale that decides how money is written, and its region, from the
/// environment in the order POSIX gives them precedence
pub fn locale() -> Option<(String, String)> {
	let locale = ["LC_ALL", "LC_MONETARY", "LANG"]
		.iter()
		.find_map(|name| std::env::var(name).ok().filter(|v| !v.is_empty()))?;
	let region = region_of(&locale)?;
	Some((locale, region))
}

/// The currency people use in a region
pub fn currency_for_region(region: &str) -> Option<&'static str> {
	CURRENCIES
		.iter()
		.find(|(_, regions)| regions.contains(&region))
		.map(|(code, _)| *code)
}

/// A currency name ledr can read, or why not
pub fn check_currency(name: &str) -> Result<String, String> {
	let name = name.trim();
	if name.is_empty() {
		return Err("Name a currency, like USD or EUR".into());
	}
	if name.contains(char::is_whitespace) {
		return Err("A currency is one word, like USD".into());
	}
	if let Some(c) = name.chars().find(|c| "#\"{}@,".contains(*c)) {
		return Err(format!("A currency can't contain `{c}`"));
	}
	if crate::syntax::lexer::is_number(name) {
		return Err("That's a number; name a currency, like USD".into());
	}
	Ok(name.to_uppercase())
}

fn capitalized(word: &str) -> String {
	let mut chars = word.chars();
	match chars.next() {
		Some(first) => first.to_uppercase().chain(chars).collect(),
		None => String::new(),
	}
}

/// Makes an account under `category` from what someone typed. Words run
/// together into one name, so `emergency fund` becomes
/// `Assets:EmergencyFund`, and colons nest, so `Chase:Checking` becomes
/// `Assets:Chase:Checking`. The category can be typed too, or left out.
pub fn account_name(category: &str, typed: &str) -> Result<String, String> {
	let typed = typed.trim();
	if let Some(c) = typed.chars().find(|c| "#\"{}@".contains(*c)) {
		return Err(format!("Account names can't contain `{c}`"));
	}
	let mut segments: Vec<&str> = typed.split(':').collect();
	let first = segments[0].trim();
	if let Some(top) = VALID_PREFIXES
		.iter()
		.find(|p| p.eq_ignore_ascii_case(first))
	{
		if *top != category {
			return Err(format!("This is for {category} accounts, not {top}"));
		}
		segments.remove(0);
	}
	let parts: Vec<String> = segments
		.iter()
		.map(|s| s.split_whitespace().map(capitalized).collect())
		.collect();
	if parts.is_empty() || parts.iter().any(String::is_empty) {
		return Err(format!("Name the account, like {category}:Checking"));
	}
	Ok(format!("{category}:{}", parts.join(":")))
}

/// Where a ledger typed as `typed` would be: `~` is the home directory, a
/// relative path is from here, and a directory gets a `main.ledr` in it
pub fn resolve_path(typed: &str) -> PathBuf {
	let path = config::expand_home(typed.trim());
	let path = if path.is_relative() {
		std::env::current_dir().map_or(path.clone(), |here| here.join(&path))
	} else {
		path
	};
	if path.is_dir() {
		path.join("main.ledr")
	} else {
		path
	}
}

/// Why a new ledger can't be created at `path`, if it can't
pub fn check_new_path(path: &Path) -> Result<(), String> {
	let mut ancestor = path.parent();
	while let Some(dir) = ancestor {
		if dir.is_file() {
			return Err(format!(
				"`{}` is a file, so it can't hold a folder",
				config::display(dir)
			));
		}
		if dir.is_dir() {
			return Ok(());
		}
		ancestor = dir.parent();
	}
	Ok(())
}

impl Setup {
	/// A ledger with the usual accounts and categories and no balances, for
	/// someone who'd rather fill it in themselves
	pub fn usual(date: Date, currency: &str, region: Option<&str>) -> Self {
		let holdings = holdings(region);
		Self {
			date,
			currency: currency.to_string(),
			accounts: holdings[..3]
				.iter()
				.map(|name| Opening::new(format!("Assets:{name}"), None))
				.chain([Opening::new("Liabilities:CreditCard", None)])
				.collect(),
			categories: chosen("Income", INCOME)
				.chain(chosen("Expenses", EXPENSES))
				.collect(),
		}
	}

	/// The entry that sets every opening balance, if any account has one
	pub fn opening(&self) -> Option<Draft> {
		let mut postings: Vec<DraftPosting> = self
			.accounts
			.iter()
			.filter_map(|a| {
				let (value, currency) = a.balance.as_ref()?;
				(!value.is_zero()).then(|| {
					DraftPosting::new(&a.account, Some((*value, currency)))
				})
			})
			.collect();
		if postings.is_empty() {
			return None;
		}
		postings.push(DraftPosting::new(OPENING, None));
		Some(Draft {
			date: self.date,
			description: "Opening balances".into(),
			postings,
			notes: vec![],
		})
	}

	/// What everything comes to, by currency: what's held less what's owed
	pub fn net_worth(&self) -> BTreeMap<String, Quant> {
		let mut sums: BTreeMap<String, Quant> = BTreeMap::new();
		for (value, currency) in
			self.accounts.iter().filter_map(|a| a.balance.as_ref())
		{
			*sums.entry(currency.clone()).or_default() += *value;
		}
		sums
	}

	/// Every currency used, the main one first
	pub fn currencies(&self) -> Vec<String> {
		let mut out = vec![self.currency.clone()];
		for (_, currency) in
			self.accounts.iter().filter_map(|a| a.balance.as_ref())
		{
			if !out.contains(currency) {
				out.push(currency.clone());
			}
		}
		out
	}

	/// The ledger, ready to be written to a file
	pub fn to_text(&self) -> String {
		let date = self.date;
		let mut out = format!(
			"# Started with `ledr init` on {date}.\n\
			#\n\
			# Add entries with `ledr add`, or write them here yourself.\n\
			# `man 5 ledr` describes the format, and `ledr check` finds\n\
			# mistakes.\n"
		);

		let mut group = |names: Vec<String>, keyword: &str| {
			if !names.is_empty() {
				out.push('\n');
			}
			for name in names {
				out.push_str(&format!("! {date} {keyword} {name}\n"));
			}
		};
		group(self.currencies(), "currency");
		group(
			self.accounts
				.iter()
				.map(|a| a.account.clone())
				.chain([OPENING.to_string()])
				.collect(),
			"account",
		);
		for category in ["Income", "Expenses"] {
			group(
				self.categories
					.iter()
					.filter(|c| c.split(':').next() == Some(category))
					.cloned()
					.collect(),
				"account",
			);
		}

		if let Some(entry) = self.opening() {
			out.push('\n');
			out.push_str(&entry.to_text(
				&FileStyle::default(),
				&History::build(&[]),
				&decimals,
			));
		}
		out
	}
}

/// The categories from a list that start out chosen, as accounts
pub fn chosen(
	category: &'static str,
	presets: &'static [(&'static str, bool)],
) -> impl Iterator<Item = String> {
	presets
		.iter()
		.filter(|(_, on)| *on)
		.map(move |(name, _)| format!("{category}:{name}"))
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::input::expr;
	use crate::parsing::{LoadOptions, load_ledger_text};
	use crate::syntax::source::{SourceFile, SourceMap};

	fn q(s: &str) -> Quant {
		s.parse().unwrap()
	}

	fn date(s: &str) -> Date {
		Date::from_str(s).unwrap()
	}

	fn example() -> Setup {
		Setup {
			date: date("2026-09-24"),
			currency: "CAD".into(),
			accounts: vec![
				Opening::new("Assets:Chequing", Some((q("2340.12"), "CAD"))),
				Opening::new("Assets:Savings", Some((q("10000"), "CAD"))),
				Opening::new("Assets:Brokerage", Some((q("1500"), "USD"))),
				Opening::new("Assets:Cash", None),
				Opening::new("Liabilities:Visa", Some((q("-512.3"), "CAD"))),
			],
			categories: vec![
				"Income:Salary".into(),
				"Expenses:Food:Groceries".into(),
				"Expenses:Housing:Rent".into(),
			],
		}
	}

	#[test]
	fn test_to_text() {
		assert_eq!(
			example().to_text(),
			"\
# Started with `ledr init` on 2026-09-24.
#
# Add entries with `ledr add`, or write them here yourself.
# `man 5 ledr` describes the format, and `ledr check` finds
# mistakes.

! 2026-09-24 currency CAD
! 2026-09-24 currency USD

! 2026-09-24 account Assets:Chequing
! 2026-09-24 account Assets:Savings
! 2026-09-24 account Assets:Brokerage
! 2026-09-24 account Assets:Cash
! 2026-09-24 account Liabilities:Visa
! 2026-09-24 account Equity:OpeningBalances

! 2026-09-24 account Income:Salary

! 2026-09-24 account Expenses:Food:Groceries
! 2026-09-24 account Expenses:Housing:Rent

2026-09-24 Opening balances
    Assets:Chequing    2,340.12 CAD
    Assets:Savings    10,000.00 CAD
    Assets:Brokerage   1,500.00 USD
    Liabilities:Visa    -512.30 CAD
    Equity:OpeningBalances
"
		);
	}

	/// What `ledr init` writes must be a ledger ledr reads without
	/// complaint, and one `ledr tidy` would leave alone
	fn assert_sound(setup: &Setup) {
		let text = setup.to_text();
		let mut sources = SourceMap::new();
		let options = LoadOptions::new("new.ledr");
		let loaded = load_ledger_text(
			Path::new("new.ledr"),
			text.clone(),
			&options,
			&mut sources,
		)
		.unwrap_or_else(|e| panic!("{e}\n{text}"));
		assert!(loaded.ledger.warnings.is_empty(), "{text}");

		let file = sources.add(SourceFile::new("t".into(), text.clone()));
		assert_eq!(crate::tidy::tidy(&text, file).unwrap(), text);
	}

	#[test]
	fn test_setups_are_sound_ledgers() {
		assert_sound(&example());
		let today = date("2026-09-24");
		for region in [None, Some("CA"), Some("US"), Some("GB"), Some("AU")] {
			assert_sound(&Setup::usual(today, "EUR", region));
		}
		let mut everything = Setup::usual(today, "JPY", None);
		everything.accounts[0].balance = Some((q("1234567"), "JPY".into()));
		everything.categories = INCOME
			.iter()
			.map(|(n, _)| format!("Income:{n}"))
			.chain(EXPENSES.iter().map(|(n, _)| format!("Expenses:{n}")))
			.collect();
		assert_sound(&everything);
		assert!(everything.to_text().contains("1,234,567 JPY"));
	}

	#[test]
	fn test_nothing_to_open_means_no_entry() {
		let setup = Setup::usual(date("2026-01-01"), "USD", Some("US"));
		assert_eq!(setup.opening(), None);
		assert!(!setup.to_text().contains("Opening balances"));
		// The account is still there for a balance added later
		assert!(setup.to_text().contains("account Equity:OpeningBalances"));
	}

	#[test]
	fn test_net_worth() {
		let worth = example().net_worth();
		assert_eq!(worth["CAD"], q("11827.82"));
		assert_eq!(worth["USD"], q("1500"));
	}

	#[test]
	fn test_account_name() {
		let name = |typed| account_name("Assets", typed);
		assert_eq!(name("Checking").unwrap(), "Assets:Checking");
		assert_eq!(name(" emergency fund ").unwrap(), "Assets:EmergencyFund");
		assert_eq!(name("chase:checking").unwrap(), "Assets:Chase:Checking");
		assert_eq!(name("RBC Chequing").unwrap(), "Assets:RBCChequing");
		assert_eq!(name("assets:Cash").unwrap(), "Assets:Cash");
		assert_eq!(name("401k").unwrap(), "Assets:401k");
		assert!(name("Liabilities:Visa").unwrap_err().contains("Assets"));
		assert!(name("Assets").is_err());
		assert!(name("Bank::Checking").is_err());
		assert!(name("Visa #2").unwrap_err().contains('#'));
		assert!(name("").is_err());
	}

	#[test]
	fn test_locales() {
		assert_eq!(region_of("en_CA.UTF-8").as_deref(), Some("CA"));
		assert_eq!(region_of("de_DE@euro").as_deref(), Some("DE"));
		assert_eq!(region_of("en-gb").as_deref(), Some("GB"));
		assert_eq!(region_of("C"), None);
		assert_eq!(region_of("C.UTF-8"), None);
		assert_eq!(currency_for_region("CA"), Some("CAD"));
		assert_eq!(currency_for_region("FR"), Some("EUR"));
		assert_eq!(currency_for_region("ZZ"), None);
	}

	#[test]
	fn test_currency_of() {
		let of = |input: &str, main| {
			currency_of(input, &expr::parse(input).unwrap(), main)
		};
		assert_eq!(of("20", "CAD"), "CAD");
		assert_eq!(of("$20", "CAD"), "CAD");
		assert_eq!(of("$20", "EUR"), "USD");
		assert_eq!(of("20 usd", "CAD"), "USD");
		assert_eq!(of("¥500", "CNY"), "CNY");
		assert_eq!(of("€20", "CAD"), "EUR");
	}

	#[test]
	fn test_check_currency() {
		assert_eq!(check_currency(" cad ").unwrap(), "CAD");
		assert!(check_currency("US D").is_err());
		assert!(check_currency("12").is_err());
		assert!(check_currency("").is_err());
	}

	#[test]
	fn test_paths() {
		let dir = tempfile::tempdir().unwrap();
		let path = dir.path();
		assert_eq!(
			resolve_path(path.to_str().unwrap()),
			path.join("main.ledr")
		);
		let file = path.join("a.ledr");
		std::fs::write(&file, "").unwrap();
		assert_eq!(resolve_path(file.to_str().unwrap()), file);
		assert!(check_new_path(&path.join("new/deeper/b.ledr")).is_ok());
		assert!(check_new_path(&file.join("b.ledr")).is_err());
	}
}
