LEDR(1)

# NAME

ledr - plain text accounting tool

# SYNOPSIS

*ledr* -f FILE _command_

# DESCRIPTION

*ledr* is a plain text accounting tool. Its primary function is to interpret
and summarize text files containing records of movements of money between
accounts. These movements may be actual (i.e. the purchase of goods) or
abstract (i.e. the appropriation of funds for a specific purpose). In other
words, it is a reporting tool from a text-based *general ledger*.

In addition to this primary function, *ledr* contains an importer that can
pull transactions from external financial systems into a specified text file
in the proper format. At time of writing, the only import target is Mercury,
the financial services firm in the United States. This makes *ledr* highly
suited to those individuals and companies whose primary banking is done there.
Additional importers may exist in the future.

*ledr* contains a number of modules for different purposes. The three
categories of modules are (1) financial statements; (2) lot reports, and (3)
auxiliary functions. Each is described below in turn. All of these expect
an input file in the _ledr_(5) format. A number of configuration options also
exist to modify the display of reports.

With the exception of the _import_ command, *ledr* is solely a read-only tool,
and does not modify files. The _import_ command is only capable of appending
to the end of the file it targets.

# FINANCIAL STATEMENTS

## ledr bs

Balance sheet. Displays a summary of accounts in the Assets, Liabilities and
Equity categories. A point-in-time report that takes into account all entries
prior to the specified end date, to show a complete balance as of that date.

## ledr is

Income statement. Displays a summary of accounts in the Income and Expenses
categories. This report accepts a time-range, to indicate the total income and
expenses during a certain period.

## ledr tb

Trial balance. Displays a summary of all accounts across all five categories.
Generally used to verify that a ledger balances, or to get an overview of its
complete contents.

# LOT REPORTS

## ledr rgl

Realized gains and losses. Displays a table containing all lot sales in the
given interval, with cost basis information for each, proceeds information if
available, and (if both are available), the net unit and total gain/loss for
each lot.

## ledr ugl

Unrealized gains and losses. Displays a table containing all open lots and
their associated quantities, cost bases and, if exchange rates are available,
estimations of their present value and the net unit and total unrealized
gain/loss for each lot.

## Remarks

Lot reports rely on the entry of data in certain ways to make sure *ledr* can
correctly interpret the proceeds from a given lot sale. For detailed discussion
on this topic, consult _ledr_(7).

# AUXILIARY FUNCTIONS

This section contains all commands that do not fit into either of the above
categories.

## ledr er

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

## ledr as <ACCOUNT>

Account summary. Given an account name, prints a summary of all entries that
affected the balance of that account, and the net impact on that account.
Accepts partial name matches, and therefore can be used to summarize a specific
account or a group of accounts that share part of their name.

## ledr fmt

Format command. In this mode, *ledr* processes the ledger file as normal, then
outputs the final computed results to standard output in a standard format.
This can be used to validate *ledr* understood an entry correctly, for
debugging, or to sanitize the formatting of the ledger file.

## ledr find <TERM>

Find command. Reports a list of all entries whose descriptions match the given
search term in a standard format. Like *ledr fmt*, but filtered.

## ledr check

Check command. Processes the ledger and reports possible common data quality or
integrity problems to standard output. Most of the things this command checks
for relate to lot management and currency conversion. Additional functions of
this command are a useful point of feedback. The intention is for it to provide
a useful way to validate a ledger file's data accuracy.

## ledr import

Import command. For documentation on this command, consult _ledr-import_(1).

# FLAGS

Almost all *ledr* commands support flags to modify their behavior in useful
ways. This section lists all supported flags.

## -f <FILE>

Specifies the input file. Required.

## -b <DATE> / --begin <DATE>

Ignore entries prior to the given date. Ignored for balance sheet and open lot
reports such as unrealized capital gains and losses.

## -e <DATE> / --end <DATE>

Ignore entries after the given date. Always respected. This allows you to
produce any report as though you were processing the ledger on a specific date
in the past.

## --config <CONFIG>

Indicate a custom config file location. The default is
_~/.config/ledr/config.toml_. The config file is only used for the *import*
command, and is otherwise ignored. You can safely ignore this flag if you don't
intend to use the importer.

For more details, first consult _ledr-import_(1).

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

## -h / --help

Prints a concise guide to the usage of *ledr*.

## -V / --version

Prints the version of *ledr* that you have installed.

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

_ledr-import_(1) - ledger transaction importer documentation

# DISCLAIMER

*ledr*'s maintainers are not financial experts and *ledr* is not a substitute
for professional financial advice. *ledr* does not provide financial advice.
*ledr* does not provide tax advice. *ledr*'s maintainers encourage you to
consult qualified financial professionals for such advice.

# AUTHORS

Maintained by *Adam Train <adam@trainrelay.net>*. Up-to-date source code can be
found at *github.com/adamexists/ledr* and contributions are accepted via GitHub.

# FIN

Here's what this doc should contain in the future:

- If multiple lots are reported at the same date, all buys are assumed to
  come before all sells. Logic with lots is FIFO by default.
- Include instructions are not directives. They have different syntax. It
  is best practice to use fully qualified paths with them, else it'll be
  relative to the current working directory. Document all directives too.
- If two or more implicit exchange rates are inferred from the same date,
  each will be averaged.
- If you want to short something, then that's a lot with a negative cost basis
- We allow for entries that may not make sense at first glance, like if there
  are two lines in different currencies that are both positive. This actually
  implies a negative exchange rate between the two currencies, as ledr will
  force the two lines to balance. This is useless for most people, but it
  makes sense in cases like shorting a stock (keeping track of your shorted
  share counts), so we allow it.
- If you are using ledr to track lots, you are highly encouraged to record
  sale transactions individually, so the proceeds from them can be
  automatically tracked for reports. If you do not isolate these with just
  one line item (the proceeds from sale), then some lot reports will not
  contain complete information.
- Generally establish that directives bind input, i.e. that a rate directive,
  for example, will make ledr reject input that deviates too far from it on
  that date.
- Entries are processed in the order they are written in the file, taking
  account of directives (all processed first) and include statements (which
  halt in the current file and process the next). Describe the control flow.
- Describe how highly different currency conversions on the same day can yield
  unexpected results and significant conversion error. Also, describe rounding
  error in similar form.
