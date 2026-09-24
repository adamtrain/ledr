LEDR(1)

# NAME

ledr - plain text accounting tool

# SYNOPSIS

*ledr* [-f _FILE_] [_options_] [_command_ [_arguments_]]

# DESCRIPTION

*ledr* is a plain text accounting tool. Its primary function is to interpret
and summarize text files containing records of movements of money between
accounts. These movements may be actual (i.e. the purchase of goods) or
abstract (i.e. the appropriation of funds for a specific purpose). In other
words, it is a reporting tool from a text-based *general ledger*.

*ledr* contains a number of modules for different purposes. The four
categories of modules are (1) financial statements; (2) lot reports; (3)
writing entries, and (4) auxiliary functions. Each is described below in
turn. All of these expect an input file in the _ledr_(5) format. A number of
configuration options also exist to modify the display of reports.

*ledr* only ever changes files when asked to: *ledr init* creates a ledger
and records where it is, *ledr add* appends entries, and *ledr tidy --write*
reformats files. Every other command only reads.

Run with no command, *ledr* shows an overview of the ledger: net worth, the
last two months of income and spending, and the most recent entries. With no
ledger to read yet, it suggests *ledr init*.

# OUTPUT

In a terminal, reports are _fancy_: colored, with summary panels, account
trees, proportional bars and charts. When output goes anywhere else, such as a
pipe or a file, reports are _plain_: simple text in a stable format that is
easy to process with other tools. The *--plain* and *--fancy* flags choose
one or the other explicitly.

Errors and warnings point at the lines of the ledger they are about, as
_path_:_line_, and in a terminal show those lines with the problem marked.

# FINANCIAL STATEMENTS

## ledr bs (balance)

Balance sheet. Displays a summary of accounts in the Assets, Liabilities and
Equity categories. A point-in-time report that takes into account all entries
prior to the specified end date, to show a complete balance as of that date.

## ledr is (income)

Income statement. Displays a summary of accounts in the Income and Expenses
categories. This report accepts a time-range, to indicate the total income and
expenses during a certain period. The fancy form also shows net income and
how much of the income was saved.

## ledr tb (trial)

Trial balance. Displays a summary of all accounts across all five categories.
Generally used to verify that a ledger balances, or to get an overview of its
complete contents.

# LOT REPORTS

## ledr rgl (realized)

Realized gains and losses. Displays a table containing all lot sales in the
given interval, with cost basis information for each, proceeds information if
available, and (if both are available), the net unit and total gain/loss for
each lot. Proceeds in another currency than the cost basis are converted at
the rate known on the date of the sale.

## ledr ugl (unrealized)

Unrealized gains and losses. Displays a table containing all open lots and
their associated quantities, cost bases and, if exchange rates are available,
estimations of their present value and the net unit and total unrealized
gain/loss for each lot.

A lot with no known price shows as _UNK_ (or "no price") rather than as
breaking even, and makes its currency's total unknown too, since any sum
would be misleading. Declare a price with a rate directive to value it.

## Remarks

Lot reports rely on the entry of data in certain ways to make sure *ledr* can
correctly interpret the proceeds from a given lot sale. For detailed discussion
on this topic, consult _ledr_(7).

# WRITING ENTRIES

## ledr init [PATH]

Starts a new ledger. In a terminal, *ledr* asks where to put it (suggesting
_~/books/main.ledr_, and completing paths with tab), the main currency (guessed
from the locale), and the date the books start, usually today. Then it asks
for each account that holds money and what it held on that date, each debt and
what was owed on it, and which income and expense categories to start with.
Account names can be typed loosely: _emergency fund_ becomes
_Assets:EmergencyFund_. Balances may be arithmetic and may name a currency; a
_$_ means the main currency if that is a kind of dollar.

The ledger has a declaration for each currency and account, all dated the day
the books start, and one entry setting every opening balance against
_Equity:OpeningBalances_. *ledr* checks it as it would any ledger, creates the
file (and its folder, if need be), and saves its location in the config file
described under FILES, so that *-f* is no longer needed.

A _PATH_ that is a folder gets a _main.ledr_ in it. A file that already exists
is never changed: *ledr* offers to make it the default ledger instead.

*-y*, *--yes*
	Ask nothing: start with the usual accounts and categories and no
	balances. The main currency is the one given with *-c*, or else the
	locale's. Also makes an existing file the default without asking.

*-n*, *--dry-run*
	Show the ledger, but create nothing.

*--no-config*
	Don't make it the default ledger.

## ledr add [WORDS...]

Adds an entry to the ledger. With no words, and in a terminal, *ledr* asks for
the entry one question at a time: the date, a description (suggesting past
ones as you type), then each account (with fuzzy completion, so _chk_ finds
_Assets:Checking_) and amount. If the description was used before, it offers
to reuse that entry's accounts, so only the amount needs typing. Amounts may
be arithmetic, like _12.50+3.20_, and may name a currency, like _4.50 CAD_.
Leave one amount blank and it balances the entry.

With words, *ledr* builds the entry from them directly, using the ledger's
history to fill the gaps:

	ledr add coffee 4.50++
ledr add yesterday rent 1850++
ledr add fri "Pizza night" 23.50 @restaurants @visa++
ledr add "New bike" 340 @Expenses:Hobbies @checking

The first word may be a day: _today_, _yesterday_, or a weekday such as _fri_
(the most recent one). A date written as _03-15_ or _2024-03-15_ counts
anywhere, and is never taken as a subtraction; write _(12-3)_ to subtract. The
last number is the amount, and a currency beside it is its currency.

Words beginning with _@_ are accounts, matched against existing ones by a few
letters (_@chk_ finds _Assets:Checking_), or created if written out in full.
An account under _Assets_ or _Liabilities_ is taken as what paid, and any
other as what the money was for; with two of the former, it is a transfer to
the first from the second. Money earned under _Income_ is credited, i.e.
written as negative, as income always is.

The rest is the description, which is matched against past descriptions, and
against the accounts of past entries (so _coffee_ finds the entry last used
for _Expenses:Food:Coffee_). A matching entry is reused with the new amount,
moving the same way the old one did, and any accounts given replace the ones
with the same role; entries with more than two lines, like a paycheck, are
scaled in proportion. Entries with prices, lots or more than one currency
can't be reused this way; run *ledr add* with no words for those.

Whichever way it was written, the entry is shown, then checked against the
whole ledger as though it were already saved, and only written if it passes.
The check always covers the whole ledger, whatever *--begin*, *--end* or
*--period* say. Accounts and currencies used for the first time are declared
automatically, if the ledger declares its names. The entry is appended to the
file holding the latest entry, with its amounts aligned and its line endings
matching that file. Descriptions and accounts can't contain _#_, since it
would start a comment.

*--to* _FILE_
	Add to this file instead. It must be a file the ledger reads, i.e. the
	ledger file itself or one it includes.

*-y*, *--yes*
	Save without asking. Required when there is no terminal to ask in.

*-n*, *--dry-run*
	Show the entry, but save nothing.

## ledr tidy

Formats every file of the ledger: amounts line up on their decimal points,
spacing and indentation become consistent, dates are written in full, and
long runs of blank lines are shortened. Comments and the order of everything
stay exactly as they were, as do Windows line endings. *ledr* checks that each
tidied file means exactly what the original did, and refuses to change it
otherwise.

Files are replaced whole, so an interrupted write never leaves one half
written. A symbolic link is followed, so the file it points to is the one
tidied, and read-only files are left alone.

By default, *ledr tidy* shows the changes it would make as a diff.

*-w*, *--write*
	Rewrite the files in place.

*--check*
	List the files that are not tidy, and exit with failure if there are any;
	useful in continuous integration or a pre-commit hook.

# AUXILIARY FUNCTIONS

This section contains all commands that do not fit into either of the above
categories.

## ledr er (rates)

Exchange rates report. Prints a table containing each of the declared,
observed, or inferred exchange rates that *ledr* has interpreted from the
ledger within a given time range.

Each rate has a corresponding type. There are three types, listed below in
descending order of accuracy:

- _De_: a declared rate, i.e. a rate abstractly specified in a rate directive
- _Ob_: an observed rate, i.e. a rate directly observed as having occurred
between two currencies
- _In_: an inferred rate, i.e. a rate deduced by converting multiple times
internally using other declared or observed rates. Only possible between
currencies that have not directly traded with each other.

Note that, if two currencies that have not directly traded with each other both
have declared rates with one or more intermediary currencies (or, in other
words, if there is an unbroken chain of declared exchange rates between the
currencies), then that rate will also be reported as declared, because it must
logically follow from the data in the ledger file.

The fancy form lists only pairs of currencies that meet directly, with a chart
of each pair's history; the plain form lists every pair.

## ledr as (account) <ACCOUNT>

Account summary. Given an account name, prints a summary of all entries that
affected the balance of that account, and the net impact on that account.
Accepts partial name matches, and therefore can be used to summarize a specific
account or a group of accounts that share part of their name. The fancy form
adds a running balance, a chart of the balance over time, and the
descriptions that moved the most money. With *--begin*, earlier entries are
not listed, but still count towards the running balance.

## ledr fmt (print)

Print command. In this mode, *ledr* processes the ledger file as normal, then
outputs the final computed results to standard output in a standard format:
every line balanced, conversions made explicit. This can be used to validate
that *ledr* understood an entry correctly, or for debugging. To reformat the
ledger files themselves, use *ledr tidy*.

## ledr find <TERM>

Find command. Reports a list of all entries whose descriptions match the given
search term in a standard format. Like *ledr fmt*, but filtered. The search
ignores case, unless the term contains a capital letter.

## ledr check

Check command. Processes the ledger and reports every problem it finds at
once, rather than stopping at the first: syntax errors, undeclared or
misspelled accounts and currencies (with suggestions), entries that do not
balance, and warnings about likely mistakes. Most warnings relate to lot
management and currency conversion, such as a conversion far from the rate
declared for its date. For undeclared names, it prints the declarations that
would fix them, ready to paste.

*ledr check* exits with failure if there are errors.

*--strict*
	Exit with failure if there are warnings, too.

## ledr accounts

Lists every account, declared or used. Plain output is one account per line,
for use with other tools; fancy output adds how often and how recently each
is used.

## ledr payees

Lists every entry description, with how often each is used.

## ledr completions <SHELL>

Prints a completion script for _bash_, _zsh_, _fish_, _elvish_ or
_powershell_. For example, for zsh:

	ledr completions zsh > ~/.zfunc/\_ledr

# FLAGS

Almost all *ledr* commands support flags to modify their behavior in useful
ways. This section lists all supported flags.

## -f <FILE> / --file <FILE>

Specifies the input file, or _-_ to read the ledger from standard input.
Without it, *ledr* reads the file named by the *LEDR_FILE* environment
variable, or else the one saved in the config file by *ledr init*.

## -b <DATE> / --begin <DATE>

Ignore entries prior to the given date. Ignored for balance sheet and open lot
reports such as unrealized capital gains and losses.

Dates may be written in full (_2024-03-15_), as a month (_2024-03_) or year
(_2024_), or as _today_, _yesterday_, _tomorrow_, or a number of days ago
(_-30_). A month or year begins on its first day.

## -e <DATE> / --end <DATE>

Ignore entries after the given date. Always respected. This allows you to
produce any report as though you were processing the ledger on a specific date
in the past. Dates are written as for *--begin*, except that a month or year
ends on its last day, so *-e 2024* includes all of 2024.

## -P <PERIOD> / --period <PERIOD>

Report on one period, instead of giving *--begin* and *--end*. A period may be
any date accepted by *--begin*, a quarter (_2024-Q1_), or one of
_this-month_, _last-month_, _this-quarter_, _last-quarter_, _this-year_,
_last-year_, _ytd_ (the year to date) or _mtd_ (the month to date).

## -c <SYMBOL> / --currency <SYMBOL>

Convert all balances to this currency at most recent prevailing rates.
Currencies that cannot be converted to the requested currency will be preserved
on the report unless the *--ioc* flag is also used.

## --ioc

Ignore other currencies. This only has an effect when combined with the
*-c / --currency* flag indicated above. When used, any currencies that cannot
be converted to the requested currency will be omitted from the report.

## -E / --ignore-equity

Ignore equity accounts. Equity accounts tend to mean very little to individuals
doing personal accounting, so this option exists to omit them. Has no effect
on trial balance reports.

## -d / --depth <NUMBER>

Accounts have a nested structure. This option exists to limit the detail of
reports to the given "depth" from the base account. For example, if your ledger
has accounts that look like this:

	Assets
		US
			Cash++
AR++
Investments
	Liabilities
		US
			Loans
				Chase
			Cards
				Discover
		CA
			Cards
				Amex

Using a depth flag could reduce it to look like this:

	Assets
		US
	Liabilities
		US++
CA

The intention is to give you the option to summarize highly complex ledgers in
a more readable way, as desired.

## -i / --invert

Negates all values. This is most typically used for income statements; one of
the most commonly confusing quirks to newcomers to accounting is that income is
reported as negative and expenses are reported as positive, so this option
allows you to render an income statement where the amounts' signs are
intuitive.

## --lenient

By default, *ledr* expects you to declare each currency and each account
somewhere in your ledger. This option disables those checks if you are not
interested in listing them out.

## -p <NUMBER> / --precision <NUMBER>

Rounds or expands all values to the given number of decimal places. The default
precision is set on a currency-by-currency basis, and is equal to the highest
number of decimal places for that currency in the ledger. This flag overrides
all of that and forces the report to render at the number of decimal places
provided.

## --plain

Print plain output, even in a terminal. This is the output format *ledr* has
always had, and the default whenever output is not a terminal.

## --fancy

Print fancy output, even when output is not a terminal; for example, to page
through it with _less -R_.

## -h / --help

Prints a concise guide to the usage of *ledr*. Each command also has its own,
e.g. *ledr add --help*.

## -V / --version

Prints the version of *ledr* that you have installed.

# EXAMPLES

Start a ledger, answering a few questions about what you have and owe:

	ledr init

Generate a balance sheet from a ledger file:

	ledr -f ledger.ledr bs

Generate an income statement for 2024, converted to USD:

	ledr -f ledger.ledr is -P 2024 -c USD

See last month's income and spending, with income shown as positive:

	ledr is -P last-month -i

Add an entry like the last coffee, but for 4.75:

	ledr add coffee 4.75

Tidy the ledger files, then make sure they stay tidy in a pre-commit hook:

	ledr tidy --write++
ledr tidy --check && ledr check --strict

Check a ledger file for data quality issues:

	ledr -f ledger.ledr check

Search for all entries mentioning "Amazon":

	ledr -f ledger.ledr find Amazon

View account summary for all cash accounts:

	ledr -f ledger.ledr as Cash

Generate a trial balance with accounts summarized to depth 2:

	ledr -f ledger.ledr tb -d 2

View realized gains for the current tax year:

	ledr -f ledger.ledr rgl -b 2024-01-01

Format and validate a ledger file:

	ledr -f ledger.ledr fmt

# ACCOUNTING PRINCIPLES

*ledr*, like all prominent plain text accounting tools, uses double-entry
accounting principles in its design. This means that for every debit (i.e.
increase in the balance of an account) there must be a corresponding, equal
and opposite credit (i.e. decrease in the balance of an account). A general
description of accounting principles is beyond the scope of this manual, but
*ledr* will not be of much interest to you if you are not familiar with, or
interested in learning, the general principles of double-entry accounting.

## Rounding

*ledr* uses a rational number representation for all numerical values; in
other words, it represents all quantities as a fraction of two whole numbers.
This means it can handle decimal amounts with no loss of precision, so long as
the number of decimal places is reasonable and does not exceed the limits of
the data type. In practice, *ledr* has been tested with all fractional values
with numerator and denominator between 10E-19 and 10E+19. This comfortably
allows management of numbers in the quadrillions or higher, down to decimal
precision beyond the requirements of any human system.

Of course, in all accounting systems, there is a need for rounding. Suppose
you or your firm trades in foreign currency, keeps cryptocurrencies with
very small denominations, or tracks inventories of physical objects with a
need for many decimal places of precision. This does not mean you will want
the numbers on all reports to reflect that level of precision.

*ledr* uses a simple philosophy on rounding: no numbers are ever rounded off
until it comes time to show the results of its calculations to you, the user.
At that time, *ledr* will round quantities using "banker's rounding", i.e.
it rounds normally to the nearest whole number, but if a quantity falls
precisely between two whole numbers (e.g. $3.50 or $4.50 rounded to the
dollar), it will always round to the closest even number (e.g. both the above
example amounts would round to $4.00, not $3.00 or $5.00). This keeps rounding
error more stable over time.

All rounding introduces error. Whenever rounding error is introduced in a
manner that would affect the viewable report, *ledr* inserts an entry to the
_Equity:Rounding_ account to show you this. The full precision of all numbers
is always optionally available to see.

# LOT MANAGEMENT

*ledr* includes support for the tracking of lots. Those familiar with the
surrounding concepts can comfortably skip the *Background* section below.

## Background

A _lot_ is a certain quantity of a certain asset or commodity that was
purchased at a specific time for a specific amount. An investor, for example,
will hold many lots, and in general, one can say that the sum of one's open
lots that relate to a certain asset or commodity would be that investor's
_position_.

In most jurisdictions, lots are the basis of capital gain and loss calculation.
To determine how much one gained or lost in a given year through investment
activity, one looks at each of the lots that investor bought and, more
crucially, each of the lots that investor sold during that year. A lot's price
of acquisition is called that lot's _cost basis_, and the price that a lot sold
for is called the lot's _proceeds_. Other terms exist for these as well; here
we are just using common ones. In most jurisdictions, the tax liability for the
capital gain of a lot is computed by subtracting the total cost basis of lot
sales from the total proceeds of lot sales in a given year. This, in
combination with the existence of different tax rates based on the length of
time that one held a particular lot before selling it, necessitates detailed
tracking of not only the purchase and sale prices of lots, but the timing of
when any _specific_ lot was purchased in relation to the time of its sale.

This brings us to the concept of _lot assignment strategies_. The fact is that
you make certain purchases and then certain sales, when investing; the purpose
of a lot assignment strategy is to associate these purchases and sales together
in a suitable manner.

In practice, this may be done as described: on a detailed, lot-by-lot basis,
in which case the investor will keep track of specific lots, when they were
purchased and sold, and assign certain sales to certain specific purchases.
Operating this way is more work, but can yield tax benefits for the savvy
investor. For those who trade too many lots for this to be practical, or such
entities as gigantic corporations who only have so much to gain on their tax
bill relative to the level of effort required to specifically assign lots,
a shortcut strategy may be used. These vary throughout the world, but by far
the most common one is FIFO (first-in-first-out), meaning that one assumes,
at the time of a lot sale, that one is first selling the oldest applicable
lots still held. This maximizes the time a certain lot is considered to have
been held, which is often the point: avoiding long-term capital gains rates.

## Implementation

*ledr* supports FIFO by default, and also supports giving lots specific names
at the time of purchase and then referencing those at the time of sale, which
forces the assignment of a specific lot to a given sale. From this information,
provided good practices and good data hygiene up-front, *ledr* can compute
realized and unrealized capital gains reports to assist in the estimation of
capital gains and losses over a given interval, or one's current financial
position. (On this matter, please see the Disclaimer at the bottom of this
document.)

When multiple lot transactions occur on the same date, all buys are processed
before all sells. This ensures that lots purchased on a given date are available
for sale on that same date.

Short positions are supported via negative cost basis. If you wish to track a
short position, record it as a lot with a negative cost basis value.

For accurate proceeds tracking in lot reports, it is best practice to record
each lot sale as an isolated entry with just the sale line and the proceeds.
This allows *ledr* to unambiguously determine the proceeds for capital gains
calculations.

For further details on the syntax required to declare and assign a name to a
lot, consult _ledr_(5).

# CURRENCY CONVERSION

*ledr* is a multicurrency system, and supports multicurrency transactions.
In general, all currency amounts must independently balance. *ledr* handles
this by inserting entries to the _Equity:Conversions_ account whenever a
currency conversion takes place.

Internally, *ledr* constructs a graph data structure containing all currencies
and their exchange rates to each other. *ledr* then estimates the value of one
currency in terms of another by traversing this graph. This means *ledr* is
able to be perfectly accurate relative to imported data for direct conversions
and perform reasonable estimations of value in indirect situations, like when
you buy a lot in one currency and sell it in another, or when you have
entered exchange rates between currency A to B, B to C, and C to D, but then
currencies A and D directly trade.

Currency conversion is, in many ways, an art, not a science, from an accounting
perspective. It is also the only significant place in which loss of precision
occurs due to fluctuating rates, fees, and rounding error.

For details on the syntax required to declare exchange rates and the
circumstances in which *ledr* infers them, consult _ledr_(5). For further
discussion of how to best handle cases of complex currency conversion, consult
_ledr_(7).

# SEE ALSO

_ledr_(5) - general ledger file format

_ledr_(7) - data integrity guidance, further discussion of lot management and
currency conversion

# ENVIRONMENT

*LEDR_FILE*
	The ledger file to read when *-f* is not given. It takes precedence over
	the config file.

*LEDR_PLAIN*
	If set to anything but _0_, _false_ or _no_, output is plain unless
	*--fancy* is given.

*NO_COLOR*
	If set and not empty, fancy output has no color, following the
	convention at _no-color.org_.

*COLORTERM*
	If _truecolor_ or _24bit_, colors are exact; otherwise *ledr* uses the
	nearest of 256 colors.

*COLUMNS*
	The width to lay out fancy output in, when it cannot be detected.

*XDG_CONFIG_HOME*
	Where the config file is kept, if set; see FILES.

*LC_ALL*, *LC_MONETARY*, *LANG*
	The first of these that is set is the locale *ledr init* guesses the
	main currency from, e.g. _CAD_ for _en_CA.UTF-8_.

# EXIT STATUS

*ledr* exits 0 on success, and 1 if an error occurs, if *ledr check* finds
errors (or warnings, with *--strict*), or if *ledr tidy --check* finds files
that are not tidy. Invalid command line arguments exit with 2.

Common error conditions include invalid file format, unbalanced entries,
undeclared accounts or currencies (in strict mode), and invalid lot
references.

# FILES

_~/.config/ledr/config.toml_
	Settings kept between runs, in TOML. It lives under *XDG_CONFIG_HOME*
	instead if that is set. *ledr init* writes it, and there is one setting:

	*file* = _PATH_
		The ledger to read when neither *-f* nor *LEDR_FILE* is given. A
		leading _~_ is the home directory, and a relative path is relative to
		the config file.

	*ledr init* changes only this setting, keeping any comments. Any other
	setting is an error, shown with the line it's on, so a typo can't go
	unnoticed. All other options are given as flags or in the environment.

Ledger files conventionally use the _.ledr_ extension, though any file
extension is accepted.

# DISCLAIMER

*ledr*'s maintainers are not financial experts and *ledr* is not a substitute
for professional financial advice. *ledr* does not provide financial advice.
*ledr* does not provide tax advice. *ledr*'s maintainers encourage you to
consult qualified financial professionals for such advice.

# AUTHORS

Maintained by *Adam Train <adam@adametrain.com>*. Up-to-date source code can be
found at *github.com/adamtrain/ledr* and contributions are accepted via GitHub.
