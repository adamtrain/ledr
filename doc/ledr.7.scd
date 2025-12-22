LEDR(7)

# NAME

ledr - data integrity guidance

# DESCRIPTION

This manual covers advanced topics for *ledr* users: data integrity best
practices, detailed lot management behavior, and currency conversion mechanics.
It is intended as a companion to _ledr_(1) and _ledr_(5), providing deeper
insight into how *ledr* processes data and how to structure ledger files for
accurate reporting.

# FILE PROCESSING

*ledr* processes ledger files using a two-pass architecture. Understanding this
is useful for organizing complex ledger files.

## Two-Pass Architecture

*ledr* reads each file twice:

1. *First pass*: Processes all directives (lines beginning with '!') and include
statements. This establishes the set of valid accounts, currencies, exchange
rates, and other declarations.

2. *Second pass*: Processes all entries, detail lines, and reference lines.

This design means directives can appear anywhere in the file. An account
declaration at the end of a file is just as valid as one at the beginning.

## Include Handling

When *ledr* encounters an include statement, it pauses processing of the current
file, processes the included file (in whichever pass is active), then resumes
the original file.

Include paths are resolved relative to the current working directory, not the
directory of the file containing the include statement. For this reason, it is
best practice to use fully qualified (absolute) paths in include statements.

Circular includes are detected and rejected with an error. If file A includes
file B, and file B includes file A, *ledr* will stop processing.

## Entry Ordering

Entries on the same date are processed in the order they appear in the file.
*ledr* assigns an internal index to each entry as it is parsed, preserving file
order for entries sharing a date.

When entries are sorted (as they are internally before processing), the sort
order is:

1. Date (ascending)
2. File index (ascending, preserving file order)
3. Description (alphabetical, as a tiebreaker)

# LOT MANAGEMENT

*ledr* tracks lots for capital gains reporting. This section details the
mechanics of lot tracking.

## FIFO Default

When selling a lot without specifying which lot to sell, *ledr* uses FIFO
(first-in-first-out) assignment. Lots are ordered by acquisition date, and the
oldest lot with a matching cost basis is sold first.

## Same-Date Processing Order

When multiple lot transactions occur on the same date, all buys are processed
before all sells. This ensures that lots purchased on a given date are available
for sale on that same date if needed.

## Named Lots

Lots can be given names at purchase time using the syntax:

	Assets:Investments    30 AAPL { 234.56 USD, "my lot name" }

When selling, referencing the same name forces assignment to that specific lot:

	Assets:Investments   -30 AAPL { 234.56 USD, "my lot name" }

Named lots bypass FIFO ordering entirely. This is useful when you need specific
lot assignment for tax purposes.

If you do not name a lot, *ledr* assigns an internal numeric ID. These IDs can
change if entries are reordered or new entries are inserted on earlier dates,
so referencing them directly is discouraged.

## Cost Basis Matching

When selling a lot, the cost basis specified in the sale must match an existing
open lot. If no lot exists with that cost basis, *ledr* will report an error.
You cannot sell what you do not have.

## Partial Sales

Lots can be partially sold. If you own 100 shares and sell 30, the lot remains
open with 70 shares. Only when the quantity reaches zero is the lot marked as
closed.

## Shorts and Negative Cost Basis

*ledr* supports tracking short positions. A short is represented as a lot with
a negative cost basis. The system allows this because shorting is essentially
holding a liability in a commodity.

If an entry has two detail lines in different currencies that are both positive
(or both negative), this implies a negative exchange rate. This is unusual but
valid for cases like tracking short positions.

## Proceeds Inference

When selling a lot, *ledr* can infer the proceeds from the entry balance. For
accurate proceeds tracking in reports, it is best practice to record each lot
sale as an isolated entry with just two lines: the lot sale and the proceeds.

For example:

	2024-11-24 Sell AAPL
		Assets:Investments   -30 AAPL { 234.56 USD }
		Assets:Cash         7500.00 USD

This makes it clear that the proceeds were $7,500. If the sale is part of a
larger entry with multiple detail lines, *ledr* will attempt to infer proceeds
but may not be able to determine them accurately.

# CURRENCY CONVERSION

*ledr* supports multiple currencies and performs conversions between them using
an internal exchange rate graph.

## Rate Types

*ledr* recognizes three types of exchange rates, in descending order of
authority:

1. *Declared*: Rates explicitly set via a rate directive. These are considered
authoritative.

2. *Direct*: Rates observed from actual transactions in the ledger. When you
record an entry that exchanges one currency for another, *ledr* observes the
implied rate.

3. *Inferred*: Rates computed by traversing the rate graph through intermediary
currencies. If you have rates for USD to EUR and EUR to GBP, *ledr* can infer
a USD to GBP rate.

## Rate Graph

Internally, *ledr* builds a graph where currencies are nodes and exchange rates
are edges. When converting between currencies that have not traded directly,
*ledr* uses breadth-first search to find the shortest path through intermediate
currencies.

When multiple paths of equal length exist, *ledr* prefers paths with more
declared rates (as they are considered more reliable) and averages the
resulting conversion rates.

## Declared Rate Rules

Only one declared rate is allowed between any pair of currencies on a given
date. Attempting to declare multiple rates for the same pair on the same date
will result in an error.

Declared rates are bidirectional. Declaring that 1 USD = 1.3 CAD automatically
implies that 1 CAD = 0.769 USD (approximately).

## Inferred Rate Rules

When *ledr* observes an implicit exchange rate from a transaction, only the
first observation for a given currency pair on a given date is retained.
Subsequent observations on the same date are ignored.

This means the order of entries in your ledger file can affect which implicit
rate is used. If you have multiple transactions between the same currencies on
the same date with different effective rates, the first one processed wins.

## Implicit Conversions

When an entry has unbalanced amounts in two currencies and no virtual detail
line (a line with no amount), *ledr* creates balancing entries through the
*Equity:Conversions* account. This keeps each currency independently balanced
while recording the conversion.

## Rate Consistency

*ledr* checks for consistency within each date's rate graph. If the rates form
cycles that do not multiply to approximately 1 (within 5% tolerance), *ledr*
will warn about the inconsistency.

For example, if your rates imply: 1 USD = 1.3 CAD, 1 CAD = 0.6 EUR, and
1 EUR = 1.4 USD, multiplying these gives 1.092, which is within tolerance.
But if the EUR to USD rate were 1.6, the product would be 1.248, and *ledr*
would warn about the inconsistent cycle.

# DATA INTEGRITY BEST PRACTICES

The following practices help maintain accurate ledger data:

## Use Explicit Declarations

Run *ledr* without the --lenient flag. This requires you to declare all accounts
and currencies before use, catching typos and ensuring consistent naming.

## Use Named Lots for Important Positions

When tracking investments where specific lot identification matters (for tax
purposes), name your lots at purchase time. This prevents ambiguity when
selling.

## Isolate Lot Sales

Record each lot sale as a separate entry with just the sale and the proceeds.
This allows *ledr* to accurately track proceeds for capital gains reporting.

## Use Absolute Paths in Includes

Since include paths are resolved relative to the current working directory,
using absolute paths ensures your ledger files work regardless of where you
run *ledr* from.

## Declare Exchange Rates

For currencies you hold but trade infrequently, use rate directives to establish
current values. This improves the accuracy of unrealized gains reports.

# SEE ALSO

_ledr_(1) - command reference

_ledr_(5) - general ledger file format

# DISCLAIMER

*ledr*'s maintainers are not financial experts and *ledr* is not a substitute
for professional financial advice. *ledr* does not provide financial advice.
*ledr* does not provide tax advice. *ledr*'s maintainers encourage you to
consult qualified financial professionals for such advice.

# AUTHORS

Maintained by *Adam Train <adam@usdocument.org>*. Up-to-date source code can be
found at *github.com/aetxyz/ledr* and contributions are accepted via GitHub.
