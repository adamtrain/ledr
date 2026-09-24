LEDR(5)

# NAME

ledr - plain text accounting tool

# CONCEPTS

A *ledr* ledger file represents a set of financial transactions, some actual
and some abstract. These transactions are for some quantity of money, with
a numerical amount and a currency of exchange. Optionally, an amount may
contain a cost basis, in the case of lots.

The structure of an account is a segmented string indicating its position in a
hierarchy of accounts. For example, in general you would see accounts like:

	Assets:Cash++
Expenses:Food:Groceries++
Income:Salary:Google++
Liabilities:Loans:Mortgages:PNC

And so on. The colon (':') is used to separate segments of these accounts into
descriptive subcategories. As an individual, the structure of these accounts is
entirely up to you and is dependent on what you find useful to track, and to
what level of detail. As an organization, often you are subject to certain
standardized accounting practices such as GAAP. Such discussion is out of scope
for our purposes here.

The purpose of accounting ledgers is to record the movement of money for some
purpose. Thus, all entries into an accounting ledger must balance: they must
reflect the same amount of money leaving some account or set of accounts as the
sum that enters one or more other accounts.

In ledr, we record an entry like this:

	2024-11-24 Paycheck++
	Assets:Cash      2030.00 USD++
	Income:Salary

This reflects the movement of some external income into one's own cash account.
As you can see, you can omit up to one line from the entry. This indicates that
the unbalanced amount you've created from the debit to your Assets:Cash account
should be handled by offsetting the Income:Salary account. (Income is reported
as a negative number in standard accounting.) If I wanted to, I could've written
a negative amount manually on that entry, like this:

	2024-11-24 Paycheck++
	Assets:Cash      2030.00 USD++
	Income:Salary   -2030.00 USD

This has the same meaning. If I were to write some other number, such as this:

	2024-11-24 Paycheck++
	Assets:Cash      2030.00 USD++
	Income:Salary   -2000.00 USD

That would be invalid, because I am saying I moved $2,000 from one account and
somehow became $2,030 in another account. Nonsense! All entries must balance.

Entries may be as long and complicated as you wish. Typically, entries are a
collection of conceptually related debits and credits. For example, in a
detailed realistic entry of one's income, you may see:

	2024-11-24 Paycheck++
	Assets:Cash                2030.00 USD++
	Expenses:Taxes:SS            38.00 USD++
	Expenses:Taxes:Medicare      30.00 USD++
	Expenses:Benefits:Insurance 100.00 USD++
	Income:Salary

And so on. How to organize these is also a conceptual exercise for you,
depending on what you wish to track.

The remainder of this document serves as a complete reference for the syntax of
*ledr* ledger files.

# SYNTAX

A *ledr* ledger file is comprised of four types of constructs:

1. _Declarations_

2. _Includes_

3. _Entries_

4. _Comments_

Each is described in turn. For details about how *ledr* parses files, see the
INCLUDES section.

# DECLARATIONS

A declaration line indicates something to be true. It has the following syntax:

	! 2024-11-24 keyword [further-keyword]

It begins with an exclamation point, followed by a date in YYYY-MM-DD format,
followed by a keyword to indicate what type of declaration it is. Some
declaration types have additional keywords required.

A description of all declaration types follows, with examples.

## account <NAME>

	! 2024-11-24 account Assets:US:Chase:Checking

Declares that a given account exists and is open for use. The accompanying
date indicates the first date on which the account is allowed to be used. This
system is intended to avoid data entry mistakes, such as typos of account
names.

## currency <NAME>

	! 2024-11-24 currency USD

Declares that a given currency exists and is available for use. The accompanying
date indicates the first date on which the currency is allowed to be used.
This system is intended to avoid data entry mistakes, such as typos of currency
names.

## open <ACCOUNT>

	! 2024-11-24 open Assets:US:Chase:Checking

Declares that a given account is opened, as opposed to closed. It can be used
to convey the same meaning as an _account_ directive, in that a certain account
has begun to exist, but it has the added purpose of opening an account that had
previously been closed. The reverse does not hold: the account directive cannot
be used for this.

An account can be closed once opened with the _close_ directive.

## close <ACCOUNT>

	! 2024-11-24 close Assets:US:Chase:Checking

Declares that a given account is closed, and it is not allowed to be used for
transactions anymore. This is intended to make sure you do not reference an
account that no longer exists or is frozen, etc.

An account can be reopened once closed with the _open_ directive.

## rate <BASE> <QUOTE> <NUMBER>

	! 2024-11-24 rate USD CAD 1.3

Declares that, as of the given date, the given currency is worth an amount of
another currency at the given rate. This uses base-quote semantics, so the
question the given number is answering is "How many of the quote currency does
it take to buy one of the base asset?" Thus, the above example declares that 1
USD is equal to 1.3 CAD.

A declared rate is presumed to be accurate on the date it occurs. You cannot
declare more than one rate between the same pair of currencies on the same
date.

A rate directive is generally used to clarify the current value of an asset or
currency that you hold but have not transacted with recently, such as long-term
investments that you want to have a reasonably up-to-date current value on
reports. It can also be used to check your other entries on a date to make sure
they are coherent in relation to the actual exchange rate.

Using rate directives is not necessary to use *ledr* with multiple currencies.
If you have an entry between two currencies, like:

	2024-11-24 Tim Hortons++
	Expenses:Coffee               12.00 CAD++
	Assets:US:Chase:Checking     -10.00 USD

Then *ledr* will implicitly convert the rate for you.

For more details on *ledr*'s implementation of currency conversion, consult
_ledr_(7).

## clear <CURRENCY>

	! 2024-11-24 clear EUR

Declares that details related to the given currency on or prior to the given
date should be ignored for the purposes of reporting. The intention is to hide
old instruments that you never want to see again from reports, without needing
to delete records of history. Could be used, for example, to quickly disregard
stock options that have expired.

Often paired with a _worthless_ directive.

## worthless <CURRENCY>

Declares that a given currency has no value in terms of any other currency, and
should not be used to convert between currencies anymore. Once declared, it
cannot be reversed, so it should only be used in cases when a certain asset or
commodity will never hold value again.

Often paired with a _clear_ directive.

# INCLUDES

An include is a special type of directive. It tells *ledr* to stop reading your
current ledger file, go and read a file at another location, then come back and
continue with the current file. It is used so you do not have to keep all your
entries for your entire financial system in one file, and can instead use a
collection of files.

An individual may use one ledger file per year, for example, and a company with
high volume may use one ledger file per entity, department, or segment.

Includes are written simply like this:

	include 2022.ledr++
include 2023.ledr

And this will make ledr read those files, in the order they appear, as though
their full content appeared in that position in the current file.

A relative path is relative to the directory of the file that includes it, so
a ledger can be moved around, or read from anywhere, without breaking. A path
containing spaces can be written in double quotes:

	include "2024 archive.ledr"

*ledr* parses files by first reading every directive, and then reading every
entry. Includes are processed with the same priority as directives, so if you
had a file that first contained some directives, then an include, then more
directives, all the directives prior to the include would be processed first,
then all the directives in the included file, then all the directives after
the include statement. (Following this, the same flow would be followed in a
second pass, but this time it would include all entries.)

If multiple includes are circular, i.e. if they instruct *ledr* to include a
file that is already being read, *ledr* will complain and quit with an error
that shows the chain of files involved. Including the same file twice from
different places is also an error, since its entries would count twice.

# ENTRIES

Entries have three types of lines: an initial entry declaration line, followed
by at least two detail lines. Reference lines for notes are optional, but also
available. Entry declarations and reference lines are simple. Details are more
complex and get their own section after this one.

## Entry Declarations

An entry declaration line is of the form

	2024-11-24 <DESCRIPTION>

In other words, it must begin with a date in YYYY-MM-DD format, followed by at
least one space, then any text description you want. A description cannot be
empty.

An entry ends at the next empty line, at the next entry declaration, or at the
end of its file. Separating entries with an empty line is conventional, and is
what *ledr tidy* and *ledr add* do.

## Reference Lines

A reference line is any line that begins with two slashes ('//'). These lines
are associated with the entry that surrounds them, and appear in some reports.
You may use these to include whatever surrounding context for a transaction
that you may want to see when the given entry appears in reports. For example:

	2024-11-24 Wire Payment++
	// INV12345-COMPANYXYZ++
	// Payment for Invoice 12345++
	Expenses:Coffee           120,000.00 CAD++
	Assets:US:Chase:Checking
		
References can be multiple lines. In reports, these lines are then combined and
separated by spaces.

# ENTRY DETAILS

An entry detail line is of the form

	<ACCOUNT> <AMOUNT> <CURRENCY> [OPTIONALS]
	
In other words, it begins with an account and follows with an amount, given in
a certain currency. This is the minimum that a detail line must contain. For
example:

	Assets:US:Chase:Checking    1232.34 USD

Amounts may use commas as thousands separators, like _1,232.34_, and may leave
out a leading zero, like _.5_. A stray comma after the currency is ignored. Commas elsewhere, such as in descriptions,
references and lot names, are kept as written. Account names cannot contain
spaces, and must begin with one of the five top-level categories: _Assets_,
_Liabilities_, _Equity_, _Income_ or _Expenses_.

Exactly one detail line per entry may omit the amount and currency. If this is
done, it is assumed that any imbalance left over by any other entries is
balanced by the account indicated in the entry with amount and currency
omitted.

A detail line can optionally include additional content after the currency. The
available forms are as follows:

## Inline Conversions

	Expenses:Shops:Hardware    1232.34 USD @ 1.3 CAD
	
This is a way of declaring an exchange rate inline. This tells *ledr* that you
paid US$1232.34, but you paid for this amount with CAD at an exchange rate of
1.3 CAD/USD. What *ledr* does internally when it sees this is to multiply the
amount of the transaction by the rate, and convert it to the currency in the
exchange detail. So, fully expanded, this becomes:

	Expenses:Shops:Hardware    1232.34 USD++
Equity:Conversions        -1232.34 USD++
Equity:Conversions         1602.042 CAD
	
It is done this way because all currencies must independently balance, so there
has to be a place to put the USD. The converted CAD amount is left as an
imbalance to be handled by some other detail in the entry, presumably
denominated in CAD.

## Inline Conversion to Total

Refer back to the above example. There are many cases when you don't have the
effective exchange rate in front of you, but you want to record a foreign
transaction in terms of what it cost you in your domestic currency. You can
rewrite the above example as:

	Expenses:Shops:Hardware    1232.34 USD @@ 1602.04 CAD

And *ledr* will interpret the amount to the right of the at-signs as a total,
rather than a rate. The full expansion is the same as above. The total is
always written as a positive number, whichever way the amount moves:

	Assets:US:Checking        -100.00 USD @@ 130.00 CAD

Whether to use this or a regular inline conversion is a matter of convenience
and preference.

## Unnamed Lot

	Assets:Investments:AAPL     30 AAPL { 234.56 USD }

This statement says "I bought 30 AAPL for 234.56 USD each, and this is a cost
basis for a lot that I just bought and now hold.

This triggers *ledr*'s investment module to pay attention to this entry as a
representation of the purchase of a lot. If you were to later sell this lot in
a future entry like this:

	Assets:Investments:AAPL     -30 AAPL { 234.56 USD }++
Assets:Cash             7500.00 USD

Then *ledr* would see that you initially paid 30 \* 234.56 USD for this lot,
but you just sold it for 7,500 USD, realizing a total profit of 463.2 USD and
thus a unit profit of 15.44 USD.

But if you were to try to sell it like this:

	Assets:Investments:AAPL     -30 AAPL { 123.45 USD }++
Assets:Cash             7500.00 USD

Then *ledr* would complain and exit with an error, because you don't have any
AAPL with that cost basis; you can't sell what you don't have! You have to
reference the correct cost basis amount for *ledr* to know _which_ AAPL you
are selling, and how to compute the net gain/loss.

*ledr* uses FIFO by default if multiple lots exist, purchased on different
dates, with the same cost basis.

## Named Lot

As just noted, *ledr* uses FIFO by default. If you need more control than this
or want to assign specific lots, you can name a lot when you purchase it:

	Assets:Investments:AAPL     30 AAPL { 234.56 USD, "lot A" }
	
Then *ledr* will remember that name when you try to sell it, if you reference
it again:

	Assets:Investments:AAPL     -30 AAPL { 234.56 USD, "lot A" }

The spacing and comma inside the braces are optional, so
_{234.56 USD "lot A"}_ means the same thing, and a name without spaces does not
need quotes.

Even if other lots came in between with the same cost basis, *ledr* would
assign the sale to the lot you named. This allows you to do any degree of
lot selection that you want; we have the benefit of working from a text
file, so you can go back and name a lot if you wish to specifically sell it,
even if you didn't name it at the time.

Internally, if you don't provide a lot with a name, *ledr* automatically
assigns an ID to it in the form of a positive integer. Referencing these at
time of sale, even if you did not initially name the lot, is possible, but
discouraged, because automatic IDs can change if the order of entries in a
ledger file is changed, or another lot is inserted later on a prior date.

# COMMENTS

Any line that begins with a '#' symbol will be ignored. Any text appearing in
any line after the first '#' symbol will be ignored. In other words, you can
put comments anywhere in the file, including on lines containing other material
that is not ignored, so long as you remember that any content at or after the
first '#' sign will be ignored.

This means the '#' sign cannot be used in the name of anything else.

Comment-only lines are fully ignored and do not count as empty lines for the
purpose of entry separation.

# MINUTIAE

A *ledr* ledger file has no specific extension. An extension of _.ledr_ is
suggested by convention.

Indentation of entry details after the first line of an entry is conventional,
but does not matter.

Whenever a space is required, *ledr* only cares that each logical element is
separated by at least one unit of whitespace. It can be any number you want,
or a tab, or anything else.

# ERRORS

This section documents common error messages and their meanings. Every error
names the file and line it is about, and most come with a suggestion for how to
fix them. *ledr check* reports all of them at once.

## Parsing Errors

*Include is missing a file path*
	The include statement is malformed. Ensure format is: include <path>

*Invalid `...` directive* / *Unknown directive `...`*
	A declaration line (starting with !) has invalid syntax, or a keyword
	*ledr* does not know (with a suggestion, if it looks like a typo).

*Entry dated ... has no description*
	A date line appears without a description after it.

*`...` is not a valid date*
	A date is not a real calendar date, or not in YYYY-MM-DD form.

*Missing a currency after `...`* / *Expected an amount after `...`*
	An entry detail line has invalid syntax. A common cause is a space in an
	account name, or a currency written before its amount.

*Unclosed `{`* / *Unexpected text after the amount*
	The conversion (@) or lot ({}) portion of a detail line is malformed.

*Posting outside of an entry*
	A detail line appears where no entry is being written, usually because
	an empty line has split an entry in two.

*Circular include*
	Two or more files include each other, creating an infinite loop.

## Declaration Errors

*Currency X declared twice*
	The same currency was declared more than once.

*Account X declared twice*
	The same account was declared more than once.

*Currency X used without declaration*
	A currency was used but never declared. Either add a currency
	declaration or use the --lenient flag.

*Account X used without declaration*
	An account was used but never declared. Either add an account
	declaration or use the --lenient flag.

*Account X is not open*
	An account was used on a date when it was closed.

## Entry Errors

*Entry does not balance: off by ...*
	The debits and credits in an entry do not sum to zero, and there is
	no virtual detail line to absorb the imbalance.

*Only one line per entry may omit amount and currency*
	Multiple detail lines in the same entry omit their amounts.

*`...` must start with one of Assets, Liabilities, Equity, Income, Expenses*
	An account does not begin with one of the five categories.

*Entry `...` has no postings*
	An entry has no detail lines.

## Lot Errors

*No remaining lots for X of Y (cost basis Z)*
	Attempting to sell more of a lot than is currently held, or
	referencing a cost basis that does not match any open lot.

## Exchange Rate Errors

*Cannot declare multiple rates between ... and ... on ...*
	Two rate directives exist for the same currency pair on the same date.

*A number is too large or too precise to compute exactly*
	*ledr* never rounds silently, so it stops rather than guess when a value
	cannot be represented exactly. Look for amounts with very many decimal
	places, or extreme exchange rates.

# SEE ALSO

_ledr_(1) - general reference

_ledr_(7) - data integrity guidance, further discussion of lot management and
currency conversion

# DISCLAIMER

*ledr*'s maintainers are not financial experts and *ledr* is not a substitute
for professional financial advice. *ledr* does not provide financial advice.
*ledr* does not provide tax advice. *ledr*'s maintainers encourage you to
consult qualified financial professionals for such advice.

# AUTHORS

Maintained by *Adam Train <adam@adametrain.com>*. Up-to-date source code can be
found at *github.com/adamtrain/ledr* and contributions are accepted via GitHub.
