# Changelog

## 3.0.0

### Fixed

These change results that earlier versions printed, so it's worth re-running reports you rely
on.

- **Realized gains** subtracted the cost basis twice when proceeds were in the cost basis
  currency, so a lot bought at 100 and sold at 150 showed a loss of 50 per unit instead of a gain.
- **Unrealized gains**: the grand total added up the lots' unit cost bases instead of their
  gains. Lots with no known price were shown with their cost as the latest price, i.e. as
  breaking even; they now show as unknown, as does the total for their currency.
- **Rounding** was wrong for fractions with an odd denominator: 2.6 rounded to 2 at zero decimal
  places, and 200/3 to 66.66 at two. Converted amounts were often affected.
- Amounts that were not already rounded, such as exchange rates and converted amounts in
  `ledr fmt`, were truncated for display rather than rounded.
- Realized gains rounded each sale's unit proceeds before multiplying out, so totals could be off
  by a few cents; they are now exact until displayed. Proceeds in another currency are converted
  at the rate on the date of the sale, rather than at the end of the report's range.
- Currencies that were only connected on different dates (bitcoin bought with dollars one day,
  swapped for ether the next) could never be converted into each other, because the graph of
  most recent rates was never filled in.
- A currency declared worthless could still be converted from, through its old rates.
- Negating zero produced a negative zero, which printed as `-0` and didn't equal zero.
- Commas were removed from descriptions, references and lot names, not just from amounts.
- Warnings about lots were sometimes printed in the middle of ordinary reports.
- Several inputs crashed ledr: `-c` with a currency only used in rate directives, amounts with
  more than 38 decimal places, and `@@` with a zero amount. Arithmetic overflow, which silently
  wrapped around in release builds, is now always a clear error.
- Error messages gave line numbers counted from zero (or from one, depending on the error),
  never named the file, and reported unbalanced entries at "line eof".
- The same file included twice was reported as a circular include.
- A total price on a negative amount, like `-10 USD @@ 13 CAD`, added the 13 CAD instead of
  taking it, and entered a negative exchange rate.
- Postings made directly to a top-level account, like `Expenses`, were counted twice in report
  totals.
- The balance sheet honoured `--begin`, contrary to its documentation, which could make balances
  wrong; a balance sheet now always counts everything up to its end.
- `--version` always said 1.0.
- The test that checks ledgers which should fail never ran, and one of those ledgers didn't fail.

### Added

- **Fancy output** in a terminal: summary panels, account trees with proportional bars, gains
  with arrows and percentages, sparklines of balances and rates, highlighted entries. Output is
  plain, exactly as before, when piped or with `--plain`; `--fancy` forces fancy output.
- **`ledr add`**: add an entry interactively, with suggestions from history, fuzzy account
  completion and arithmetic, or from a few words (`ledr add coffee 4.50`). Entries are checked
  against the whole ledger before they are saved, and new accounts are declared automatically.
- **`ledr init`**: start a ledger by answering a few questions: where to keep it, what each
  account holds, what you owe, and which categories to start with. It writes the declarations
  and an opening balances entry, and remembers the ledger in `~/.config/ledr/config.toml`, so
  `-f` is no longer needed. It never replaces a file; given an existing ledger, it only makes it
  the default.
- **`ledr tidy`**: a formatter that aligns amounts on their decimal points and keeps every
  comment, with `--write` and `--check`.
- **`ledr`** with no command shows an overview of the ledger, or with no ledger yet, how to start
  one.
- **`ledr check`** reports every problem at once, with suggestions for misspelled names and the
  declarations that would fix undeclared ones, and exits with failure on errors (and warnings,
  with `--strict`). It also warns when a conversion strays far from the rate declared that day.
- Errors show the lines they are about, point at the exact problem, and suggest fixes.
- `ledr accounts`, `ledr payees`, and `ledr completions <shell>`.
- `LEDR_FILE`, so `-f` can be left out; `-f -` reads the ledger from stdin.
- Friendly dates (`2024`, `2024-03`, `today`, `-30`) and `-P`/`--period` (`2024-Q1`,
  `last-month`, `ytd`, …).
- Long names for commands: `balance`, `income`, `trial`, `rates`, `realized`, `unrealized`,
  `account`, `print`.
- `ledr find` ignores case unless the search has capitals.
- Include paths may be quoted, and lot braces are more forgiving: `{234.56 USD "lot A"}`.
- `LEDR_PLAIN=1` makes plain output the default.

### Changed

- Only whitespace separates an entry's date from its description, so tabs work too.
- Declarations of accounts that don't start with a valid category are errors.
- An unbalanced conversion that contradicts a declared rate is now a warning in `ledr check`
  (it was silently accepted).
- Removed unused dependencies, including reqwest and its OpenSSL requirement.
- The code is split into a library and a thin binary, with a lexer shared by everything that
  reads ledger text, and report models separate from how they're drawn.
