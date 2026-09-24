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

//! Each report ledr can print: load the ledger, build the report's model,
//! and render it plainly or fancily.

use crate::config;
use crate::diagnostics::{Diagnostic, Severity, closest};
use crate::gl::entry::Entry;
use crate::gl::total::Total;
use crate::input::compose::{self, Draft, Target};
use crate::input::history::History;
use crate::input::interactive;
use crate::input::quick::{self, Quick};
use crate::input::setup::{self, Setup};
use crate::investment::lot::LotStatus;
use crate::investment::portfolio::LotFilter;
use crate::parsing::{
	LoadOptions, Loaded, load_ledger, load_ledger_text, loader,
};
use crate::render::statement::{Heading, StatementKind};
use crate::render::{self, Mode, dotted, gain_color, percent, plural};
use crate::reports::portfolio_reporter::PortfolioReporter;
use crate::reports::rate_reporter::RateReporter;
use crate::reports::register::Register;
use crate::reports::statement_reporter::StatementReporter;
use crate::syntax::source::SourceMap;
use crate::ui::style::{Style, palette};
use crate::ui::text::{Doc, Line};
use crate::ui::widgets::{Column, Decimals, Table, badge, meter, panel, rule};
use crate::util::date::Date;
use crate::util::period::parse_period;
use crate::util::quant::Quant;
use anyhow::Error;
use std::path::{Path, PathBuf};

/// Everything the command line said about how to report.
#[derive(Clone, Debug)]
pub struct Context {
	pub load: LoadOptions,
	pub mode: Mode,
	/// Convert balances to this currency where possible
	pub currency: Option<String>,
	/// With a currency, drop balances that cannot be converted
	pub ignore_other_currencies: bool,
	pub ignore_equity: bool,
	pub depth: Option<usize>,
	pub invert: bool,
	/// Whether a beginning or end date was asked for explicitly
	pub begin_given: bool,
	pub end_given: bool,
	pub today: Date,
}

/// What a command produced: its report, and anything worth mentioning
/// outside of it.
pub struct Output {
	pub text: String,
	/// Shown on stderr, so as never to get mixed into piped output
	pub notice: Option<String>,
	/// Whether the command should end in failure
	pub failed: bool,
}

impl Output {
	fn text(text: String) -> Self {
		Self {
			text,
			notice: None,
			failed: false,
		}
	}
}

impl Context {
	fn load(&self, sources: &mut SourceMap) -> Result<Loaded, Error> {
		load_ledger(&self.load, sources)
	}

	fn precision(&self) -> u32 {
		self.load.precision.unwrap_or(u32::MAX)
	}

	fn file_name(&self) -> String {
		if self.load.file == "-" {
			return "stdin".into();
		}
		Path::new(&self.load.file).file_name().map_or_else(
			|| self.load.file.clone(),
			|n| n.to_string_lossy().into_owned(),
		)
	}

	/// Faint facts about the ledger for the edge of a panel
	fn meta(&self, loaded: &Loaded, sources: &SourceMap) -> Vec<String> {
		vec![
			self.file_name(),
			if sources.len() > 1 {
				plural(sources.len(), "file")
			} else {
				String::new()
			},
			plural(loaded.ledger.entries().len(), "entry"),
		]
	}

	/// The dates a report actually covers
	fn span_of(&self, loaded: &Loaded) -> (Date, Date) {
		let entries = loaded.ledger.entries();
		let first = entries.first().map_or(self.today, |e| *e.get_date());
		let last = entries.last().map_or(self.today, |e| *e.get_date());
		(
			if self.begin_given {
				self.load.begin
			} else {
				first
			},
			if self.end_given { self.load.end } else { last },
		)
	}

	/// A footnote about warnings, pointing at `ledr check`
	fn warnings_notice(&self, loaded: &Loaded) -> Option<String> {
		let count = loaded.ledger.warnings.len();
		if count == 0 || !self.mode.is_fancy() {
			return None;
		}
		let line = Line::styled("⚠ ", Style::color(palette::AMBER))
			.with(plural(count, "warning"), Style::faint())
			.with(" · run ", Style::faint())
			.with("ledr check", Style::color(palette::ACCENT))
			.with(" for details", Style::faint());
		Some(line.render(self.mode.color()))
	}
}

pub fn statement(
	ctx: &Context,
	sources: &mut SourceMap,
	kind: StatementKind,
) -> Result<Output, Error> {
	// A balance sheet is a point in time, so it counts everything before
	// its end, whatever the beginning
	let mut options = ctx.load.clone();
	if kind == StatementKind::Balance {
		options.begin = Date::min();
	}
	let mut loaded = load_ledger(&options, sources)?;
	let meta = ctx.meta(&loaded, sources);
	let (first, last) = ctx.span_of(&loaded);
	let notice = ctx.warnings_notice(&loaded);

	let mut totals = Total::from_ledger(&loaded.ledger);
	if let Some(currency) = &ctx.currency {
		totals.collapse_to(
			currency,
			&mut loaded.ledger.exchange_rates,
			ctx.ignore_other_currencies,
		);
	}
	if ctx.invert {
		totals.invert();
	}
	let mut precision_map = loaded.result.max_precision_by_currency.clone();
	totals.round(ctx.precision(), &mut precision_map, true);

	let (mut shown, include_equity) = match kind {
		StatementKind::Balance => (vec!["Assets", "Liabilities"], true),
		StatementKind::Income => (vec!["Income", "Expenses"], false),
		StatementKind::Trial => (
			vec!["Assets", "Liabilities", "Income", "Expenses", "Equity"],
			false,
		),
	};
	if include_equity && !ctx.ignore_equity {
		shown.push("Equity");
	}
	totals.filter_top_level(shown);

	let mut reporter = StatementReporter::from_total(totals);
	reporter.sort_canonical();

	let text = match ctx.mode {
		Mode::Plain => reporter.plain(ctx.depth),
		Mode::Fancy { width, .. } => {
			let period = match kind {
				StatementKind::Income => format!("{first} → {last}"),
				StatementKind::Trial if ctx.begin_given => {
					format!("{first} → {last}")
				},
				_ => format!("as of {last}"),
			};
			let heading = Heading {
				kind,
				period,
				meta,
				inverted: ctx.invert,
				collapsed: ctx.currency.is_some(),
			};
			let doc = render::statement::statement(
				&reporter, &heading, ctx.depth, width,
			);
			ctx.mode.render(&doc)
		},
	};
	Ok(Output {
		text,
		notice,
		failed: false,
	})
}

pub fn rates(ctx: &Context, sources: &mut SourceMap) -> Result<Output, Error> {
	let loaded = ctx.load(sources)?;
	let reporter =
		RateReporter::new(loaded.ledger.exchange_rates.take_all_rates());
	Ok(Output::text(match ctx.mode {
		Mode::Plain => reporter.plain(),
		Mode::Fancy { width, .. } => ctx
			.mode
			.render(&render::rates::rates(reporter.rates(), width)),
	}))
}

pub fn realized(
	ctx: &Context,
	sources: &mut SourceMap,
) -> Result<Output, Error> {
	let loaded = ctx.load(sources)?;
	let notice = ctx.warnings_notice(&loaded);
	let end = ctx.load.end.min(ctx.today);
	let reporter = PortfolioReporter::new(
		loaded.portfolio.take_lots(vec![LotFilter::HasSales(true)]),
		loaded.result.max_precision_by_currency.clone(),
		ctx.precision(),
	);
	let report =
		reporter.realized(&ctx.load.begin, &end, &loaded.ledger.exchange_rates);
	let text = match ctx.mode {
		Mode::Plain => reporter.plain_realized(&report),
		Mode::Fancy { width, .. } => {
			let period = match (ctx.begin_given, ctx.end_given) {
				(false, false) => "all time".to_string(),
				(true, _) => format!("{} → {end}", ctx.load.begin),
				(false, true) => format!("through {end}"),
			};
			ctx.mode.render(&render::lots::realized(
				&reporter, &report, period, width,
			))
		},
	};
	Ok(Output {
		text,
		notice,
		failed: false,
	})
}

pub fn unrealized(
	ctx: &Context,
	sources: &mut SourceMap,
) -> Result<Output, Error> {
	let loaded = ctx.load(sources)?;
	let notice = ctx.warnings_notice(&loaded);
	let as_of = ctx.load.end.min(ctx.today);
	let reporter = PortfolioReporter::new(
		loaded
			.portfolio
			.take_lots(vec![LotFilter::Status(LotStatus::Open)]),
		loaded.result.max_precision_by_currency.clone(),
		ctx.precision(),
	);
	let report = reporter.unrealized(&as_of, &loaded.ledger.exchange_rates);
	let text = match ctx.mode {
		Mode::Plain => reporter.plain_unrealized(&report),
		Mode::Fancy { width, .. } => ctx.mode.render(
			&render::lots::unrealized(&reporter, &report, &as_of, width),
		),
	};
	Ok(Output {
		text,
		notice,
		failed: false,
	})
}

pub fn account(
	ctx: &Context,
	sources: &mut SourceMap,
	pattern: &str,
) -> Result<Output, Error> {
	// Load everything, so that earlier entries make the opening balance
	let mut whole = ctx.load.clone();
	whole.begin = Date::min();
	let loaded = load_ledger(&whole, sources)?;
	let register = Register::new(
		loaded.ledger.take_entries(),
		pattern,
		ctx.currency.as_deref(),
		ctx.load.begin,
	);
	Ok(Output::text(match ctx.mode {
		Mode::Plain => register.plain(),
		Mode::Fancy { width, .. } => ctx
			.mode
			.render(&render::register::register(&register, width)),
	}))
}

/// Prints entries as ledr understands them, optionally only those whose
/// description matches a search term.
pub fn print(
	ctx: &Context,
	sources: &mut SourceMap,
	term: Option<&str>,
) -> Result<Output, Error> {
	let loaded = ctx.load(sources)?;
	let entries: Vec<&Entry> = loaded
		.ledger
		.entries()
		.iter()
		.filter(|e| {
			term.is_none_or(|t| render::entries::matches(e.get_desc(), t))
		})
		.collect();

	Ok(Output::text(match ctx.mode {
		Mode::Plain => entries.iter().map(|e| format!("{e}\n")).collect(),
		Mode::Fancy { .. } => {
			if term.is_some() && entries.is_empty() {
				let term = term.unwrap_or_default();
				Line::faint(format!("No entries match “{term}”."))
					.render(ctx.mode.color())
					+ "\n"
			} else {
				let heading = term
					.map(|t| render::entries::search_heading(entries.len(), t));
				ctx.mode
					.render(&render::entries::entries(&entries, term, heading))
			}
		},
	}))
}

/// Checks the ledger thoroughly, reporting every problem found
pub fn check(
	ctx: &Context,
	sources: &mut SourceMap,
	strict: bool,
) -> Result<Output, Error> {
	let mut options = ctx.load.clone();
	options.thorough = true;
	options.keep_going = true;
	let loaded = load_ledger(&options, sources)?;

	let mut problems: Vec<&Diagnostic> =
		loaded.result.problems.iter().collect();
	problems.extend(loaded.ledger.warnings.iter());
	let errors = problems
		.iter()
		.filter(|d| d.severity == Severity::Error)
		.count();
	let warnings = problems.len() - errors;

	// Declarations that would fix undeclared names, ready to paste. A name
	// close to one already declared is a typo, not a missing declaration.
	let ledger = &loaded.ledger;
	let mut declarations: Vec<String> = vec![];
	for (currency, date) in &loaded.result.undeclared_currencies {
		if closest(currency, ledger.declared_currencies()).is_none() {
			declarations.push(format!("! {date} currency {currency}"));
		}
	}
	for (account, date) in &loaded.result.undeclared_accounts {
		if closest(account, ledger.declared_accounts()).is_none() {
			declarations.push(format!("! {date} account {account}"));
		}
	}

	let failed = errors > 0 || (strict && warnings > 0);
	let summary = match (errors, warnings) {
		(0, 0) => "No problems found".to_string(),
		(0, w) => plural(w, "warning"),
		(e, 0) => plural(e, "error"),
		(e, w) => format!("{}, {}", plural(e, "error"), plural(w, "warning")),
	};

	let text = match ctx.mode {
		Mode::Plain => {
			let mut out = String::new();
			for problem in &problems {
				out.push_str(&render::diagnostics::plain(problem, sources));
			}
			if !declarations.is_empty() {
				out.push_str("\nMissing declarations:\n");
				for d in &declarations {
					out.push_str(&format!("{d}\n"));
				}
			}
			out.push_str(&summary);
			out.push('\n');
			out
		},
		Mode::Fancy { .. } => {
			let mut doc = Doc::new();
			let (first, last) = ctx.span_of(&loaded);
			let mut facts = ctx.meta(&loaded, sources);
			if !loaded.ledger.entries().is_empty() {
				facts.push(format!("{first} → {last}"));
			}
			let rest = dotted(&facts[1..]);
			let mut heading = Line::styled("◇ ", Style::color(palette::ACCENT))
				.with(facts[0].clone(), Style::color(palette::ACCENT).bold());
			if !rest.is_empty() {
				heading.push(" · ", Style::faint());
				heading.append(rest);
			}
			doc.push(heading);
			doc.blank();
			for problem in &problems {
				doc.extend(
					render::diagnostics::fancy_width(
						problem,
						sources,
						ctx.mode.width(),
					)
					.lines,
				);
				doc.blank();
			}
			if !declarations.is_empty() {
				doc.push(
					Line::styled("✚ ", Style::color(palette::ACCENT)).with(
						"Declare these new names to fix the errors above:",
						Style::new().bold(),
					),
				);
				for d in &declarations {
					doc.push(Line::styled(
						format!("  {d}"),
						Style::color(palette::ACCENT),
					));
				}
				doc.blank();
			}
			doc.push(match (errors, warnings) {
				(0, 0) => {
					Line::styled("✓ ", Style::color(palette::GREEN).bold())
						.with(summary, Style::new().bold())
				},
				(0, _) => {
					Line::styled("⚠ ", Style::color(palette::AMBER).bold())
						.with(summary, Style::new().bold())
				},
				_ => Line::styled("✗ ", Style::color(palette::RED).bold())
					.with(summary, Style::new().bold()),
			});
			ctx.mode.render(&doc)
		},
	};

	Ok(Output {
		text,
		notice: None,
		failed,
	})
}

/// What to add, and how
#[derive(Clone, Debug, Default)]
pub struct AddRequest {
	/// A quick description, like `coffee 4.50`; empty to be asked
	pub words: Vec<String>,
	/// The file to add to, if not the one with the latest entry
	pub to: Option<String>,
	/// Save without asking
	pub yes: bool,
	/// Show the entry but don't save it
	pub dry_run: bool,
	/// Whether questions can be asked, i.e. there is a person at a terminal
	pub interactive: bool,
}

/// Adds an entry, after showing it and checking it against the whole ledger
pub fn add(
	ctx: &Context,
	sources: &mut SourceMap,
	request: &AddRequest,
) -> Result<Output, Error> {
	if ctx.load.file == "-" {
		return Err(Diagnostic::error("Can't add to a ledger read from stdin")
			.help("pass the ledger's file with -f, or set LEDR_FILE")
			.into());
	}

	// History and checks always cover the whole ledger, whatever range a
	// report would have been limited to
	let mut whole = ctx.load.clone();
	whole.begin = Date::min();
	whole.end = Date::max();
	let loaded = load_ledger(&whole, sources)?;
	let history = History::build(&loaded.items);
	let today = ctx.today;
	let target = Target::choose(
		request.to.as_deref(),
		&history,
		sources,
		&ctx.load.file,
	)?;
	let target_name = target.path.file_name().map_or_else(
		|| target.path.display().to_string(),
		|n| n.to_string_lossy().into_owned(),
	);
	let quick =
		quick::parse(&request.words, today, &|c| history.is_known_currency(c))?;
	let color = ctx.mode.color();

	let cancelled = || {
		Output::text(
			Line::faint("Cancelled. Nothing was saved.").render(color) + "\n",
		)
	};
	let ask = |start: &Quick| -> Result<Option<Draft>, Error> {
		interactive::style_prompts();
		match interactive::ask(&history, today, start) {
			Ok(draft) => Ok(Some(draft)),
			Err(e) if interactive::is_cancel(&e) => Ok(None),
			Err(e) => Err(e),
		}
	};
	let heading = Line::styled("◇ ", Style::color(palette::ACCENT))
		.with(
			format!("Adding to {target_name}"),
			Style::color(palette::ACCENT).bold(),
		)
		.with(
			format!(" · {}", plural(history.entries, "entry")),
			Style::faint(),
		);

	let draft = if request.words.is_empty() {
		if !request.interactive {
			return Err(Diagnostic::error(
				"`ledr add` needs a terminal to ask questions",
			)
			.help(
				"or describe the entry in a few words, like `ledr add coffee 4.50`",
			)
			.into());
		}
		print!("{}\n\n", heading.render(color));
		match ask(&quick)? {
			Some(draft) => draft,
			None => return Ok(cancelled()),
		}
	} else {
		match quick::resolve(&quick, &history, today) {
			Ok(draft) => draft,
			Err(problem) if request.interactive => {
				print!(
					"{}",
					render::diagnostics::error(
						&problem.into(),
						sources,
						ctx.mode
					)
				);
				println!(
					"{}\n",
					Line::faint("Let's fill in the rest.").render(color)
				);
				match ask(&quick)? {
					Some(draft) => draft,
					None => return Ok(cancelled()),
				}
			},
			Err(problem) => return Err(problem.into()),
		}
	};

	// Write it in the file's style, then check it in the context of the
	// whole ledger before anything touches the disk
	let precision = compose::precisions(&loaded);
	let block = draft.to_text(&target.style, &history, &precision);
	draft.reads_back(&block)?;
	let appendix = target.appendix(&block);
	let (checked, check_sources) = target.validate(&whole, &appendix);
	let checked = match checked {
		Ok(checked) => checked,
		Err(error) => {
			eprint!(
				"{}",
				render::diagnostics::error(&error, &check_sources, ctx.mode)
			);
			return Ok(Output {
				text: Line::faint("Nothing was saved.").render(color) + "\n",
				notice: None,
				failed: true,
			});
		},
	};
	let warnings = target.new_warnings(&checked, &check_sources, &appendix);

	// Show what is about to be saved
	let (new_accounts, new_currencies) = draft.new_names(&history);
	let mut preview = Doc::new();
	if request.interactive {
		preview.blank();
	} else {
		preview.push(heading);
	}
	for line in render::entries::highlight(&block) {
		preview.push(Line::plain("  ").then(line));
	}
	for note in &draft.notes {
		preview.push(
			Line::styled("  ↳ ", Style::faint())
				.with(note.clone(), Style::faint()),
		);
	}
	for account in &new_accounts {
		preview.push(
			Line::styled("  ✚ ", Style::color(palette::ACCENT))
				.with("new account ", Style::faint())
				.then(render::account(account)),
		);
	}
	for currency in &new_currencies {
		preview.push(
			Line::styled("  ✚ ", Style::color(palette::ACCENT))
				.with(format!("new currency {currency}"), Style::faint()),
		);
	}
	for warning in &warnings {
		preview.push(
			Line::styled("  ⚠ ", Style::color(palette::AMBER))
				.with(warning.message.clone(), Style::new()),
		);
	}
	preview.blank();
	print!("{}", preview.render(color));

	if request.dry_run {
		return Ok(Output::text(
			Line::faint("Dry run: nothing was saved.").render(color) + "\n",
		));
	}
	if !request.yes {
		if !request.interactive {
			return Err(Diagnostic::error(
				"Not saved: there's no terminal to confirm with",
			)
			.help(
				"add --yes to save without being asked, or --dry-run to only look",
			)
			.into());
		}
		match interactive::confirm(&format!("Save to {target_name}?")) {
			Ok(true) => {},
			Ok(false) => return Ok(cancelled()),
			Err(e) if interactive::is_cancel(&e) => return Ok(cancelled()),
			Err(e) => return Err(e),
		}
	}

	let line = target.append(&appendix)?;
	let saved = Line::styled("✓ ", Style::color(palette::GREEN).bold())
		.with("Saved to ", Style::new())
		.with(
			format!("{}:{line}", target.path.display()),
			Style::color(palette::ACCENT),
		);
	Ok(Output::text(saved.render(color) + "\n"))
}

/// What `ledr init` was asked to do
pub struct InitRequest {
	/// Where to create the ledger, if given
	pub path: Option<String>,
	/// Where to suggest creating it, e.g. from -f or LEDR_FILE
	pub suggested: Option<String>,
	/// The main currency, if given
	pub currency: Option<String>,
	/// Use the usual accounts rather than asking
	pub yes: bool,
	/// Show the ledger but create nothing
	pub dry_run: bool,
	/// Make it the ledger ledr reads by default
	pub remember: bool,
	/// Whether questions can be asked, i.e. there is a person at a terminal
	pub interactive: bool,
}

/// What `ledr init` settled on
enum Plan {
	/// Create a ledger here, like this
	New(PathBuf, Setup),
	/// Use the ledger already here
	Existing(PathBuf),
}

/// Starts a ledger: asks what someone has, owes and spends on, writes it
/// with opening balances, and makes it the ledger ledr reads by default.
/// A file that already exists is never touched, only offered as the
/// default.
pub fn init(
	request: &InitRequest,
	mode: Mode,
	today: Date,
) -> Result<Output, Error> {
	let color = mode.color();
	let asking = request.interactive && !request.yes;
	if !asking && !request.yes && !request.dry_run {
		return Err(Diagnostic::error(
			"`ledr init` needs a terminal to ask questions",
		)
		.help(
			"add --yes to start with the usual accounts, and add balances \
			later",
		)
		.into());
	}
	let cancelled = || {
		Output::text(
			Line::faint("Cancelled. Nothing was written.").render(color) + "\n",
		)
	};

	// Suggest the ledger ledr was told about, if it doesn't exist yet
	let remembered = config::load(&mut SourceMap::new())
		.ok()
		.flatten()
		.as_ref()
		.and_then(config::ledger)
		.filter(|f| !Path::new(f).exists());
	let suggested = request
		.suggested
		.clone()
		.filter(|f| f != "-")
		.or(remembered)
		.map_or_else(
			|| setup::DEFAULT_PATH.to_string(),
			|f| config::display(Path::new(&f)),
		);

	let planned = if asking {
		interactive::style_prompts();
		ask_plan(request, &suggested, mode, today)
	} else {
		usual_plan(request, &suggested, today)
	};
	let plan = match planned {
		Ok(plan) => plan,
		Err(e) if interactive::is_cancel(&e) => return Ok(cancelled()),
		Err(e) => return Err(e),
	};

	let (path, setup) = match plan {
		Plan::Existing(path) => return adopt(&path, request, asking, mode),
		Plan::New(path, setup) => (path, setup),
	};
	let shown = config::display(&path);
	let text = setup.to_text();

	// Check it as any ledger is checked, before it exists
	let mut sources = SourceMap::new();
	let options = LoadOptions::new(path.to_string_lossy());
	if let Err(error) =
		load_ledger_text(&path, text.clone(), &options, &mut sources)
	{
		eprint!("{}", render::diagnostics::error(&error, &sources, mode));
		return Ok(Output {
			text: Line::faint("Nothing was written.").render(color) + "\n",
			notice: None,
			failed: true,
		});
	}

	if request.dry_run {
		let mut doc = Doc::new();
		doc.push(
			Line::styled("◇ ", Style::color(palette::ACCENT))
				.with(shown, Style::color(palette::ACCENT).bold())
				.with(" would hold:", Style::faint()),
		);
		doc.blank();
		doc.extend(render::entries::highlight(&text));
		doc.blank();
		doc.push(Line::faint("Dry run: nothing was written."));
		return Ok(Output::text(mode.render(&doc)));
	}

	if asking {
		print!("{}", mode.render(&preview(&setup, mode)));
	}
	let confirmed = should_remember(&path, request, asking).and_then(|r| {
		let create =
			!asking || interactive::confirm(&format!("Create {shown}?"))?;
		Ok((r, create))
	});
	let remember = match confirmed {
		Ok((remember, true)) => remember,
		Ok((_, false)) => return Ok(cancelled()),
		Err(e) if interactive::is_cancel(&e) => return Ok(cancelled()),
		Err(e) => return Err(e),
	};

	create_new(&path, &text)?;
	let mut doc = Doc::new();
	if asking {
		doc.blank();
	}
	let names = setup.accounts.len() + setup.categories.len() + 1;
	doc.push(
		done("Created ")
			.with(shown.clone(), Style::color(palette::ACCENT))
			.with(
				format!(" with {}", plural(names, "account")),
				Style::faint(),
			),
	);
	if remember {
		doc.extend(remembered_lines(&path)?);
	}
	doc.blank();
	doc.extend(next_steps(&shown, remember, &first_add(&setup)));
	Ok(Output::text(mode.render(&doc)))
}

/// Asks everything about a new ledger, or for one that already exists
fn ask_plan(
	request: &InitRequest,
	suggested: &str,
	mode: Mode,
	today: Date,
) -> Result<Plan, Error> {
	let color = mode.color();
	let width = mode.width().min(72);
	let intro = Doc::from(vec![
		Line::styled("◆ ", Style::color(palette::ACCENT))
			.with("Let's set up your books", Style::new().bold()),
		Line::faint(
			"  A few questions, then ledr writes a ledger to build on. Nothing",
		),
		Line::faint("  is written until the end, and esc stops at any point."),
		Line::new(),
	]);
	print!("{}", intro.render(color));

	let mut typed = request.path.clone();
	let path = loop {
		let answer = match typed.take() {
			Some(path) => path,
			None => interactive::ask_path(suggested)?,
		};
		let path = setup::resolve_path(&answer);
		if !path.exists() {
			if let Err(problem) = setup::check_new_path(&path) {
				return Err(Diagnostic::error(problem).into());
			}
			break path;
		}
		let (about, readable) = match describe_ledger(&path) {
			Ok(about) => (about, true),
			Err(problem) => (
				format!("ledr couldn't read it as a ledger: {problem}"),
				false,
			),
		};
		let question = format!(
			"{} already exists. Use it as your ledger?",
			config::display(&path)
		);
		if interactive::confirm_with(&question, readable, &about)? {
			return Ok(Plan::Existing(path));
		}
	};

	let locale = setup::locale();
	let currency = match &request.currency {
		Some(given) => setup::check_currency(given).map_err(Error::msg)?,
		None => {
			let guess = locale
				.as_ref()
				.and_then(|(name, region)| {
					let code = setup::currency_for_region(region)?;
					Some((code, format!("guessed from your locale, {name}")))
				})
				.unwrap_or((
					"USD",
					"the currency most of your money is in".to_string(),
				));
			interactive::ask_currency(guess.0, &guess.1)?
		},
	};
	let date = interactive::ask_start(today)?;

	let section = |title: &str, about: &str| {
		let doc = Doc::from(vec![
			Line::new(),
			rule(title, width),
			Line::faint(about),
		]);
		print!("{}", doc.render(color));
	};
	let region = locale.as_ref().map(|(_, region)| region.as_str());
	let mut accounts = vec![];
	section(
		"Money you have",
		"Bank accounts, savings, cash: anywhere you keep money.",
	);
	interactive::ask_openings(
		"Assets",
		setup::holdings(region),
		&currency,
		date,
		&mut accounts,
	)?;
	section(
		"Money you owe",
		"Credit cards, loans, lines of credit, money borrowed from friends.",
	);
	interactive::ask_openings(
		"Liabilities",
		setup::DEBTS,
		&currency,
		date,
		&mut accounts,
	)?;
	section(
		"Categories",
		"What you earn from and spend on. More can be added any time.",
	);
	let mut categories = interactive::ask_categories(
		"Where does your money come from?",
		"Income",
		setup::INCOME,
	)?;
	categories.extend(interactive::ask_categories(
		"What do you spend it on?",
		"Expenses",
		setup::EXPENSES,
	)?);

	Ok(Plan::New(
		path,
		Setup {
			date,
			currency,
			accounts,
			categories,
		},
	))
}

/// The usual ledger, or the one already there, without asking anything
fn usual_plan(
	request: &InitRequest,
	suggested: &str,
	today: Date,
) -> Result<Plan, Error> {
	let path =
		setup::resolve_path(request.path.as_deref().unwrap_or(suggested));
	if path.exists() {
		return Ok(Plan::Existing(path));
	}
	setup::check_new_path(&path).map_err(Error::msg)?;
	let locale = setup::locale();
	let region = locale.as_ref().map(|(_, region)| region.as_str());
	let currency = match &request.currency {
		Some(given) => setup::check_currency(given).map_err(Error::msg)?,
		None => region
			.and_then(setup::currency_for_region)
			.unwrap_or("USD")
			.to_string(),
	};
	Ok(Plan::New(path, Setup::usual(today, &currency, region)))
}

/// A few words about an existing ledger, or why it can't be read
fn describe_ledger(path: &Path) -> Result<String, String> {
	let mut sources = SourceMap::new();
	let loaded =
		load_ledger(&LoadOptions::new(path.to_string_lossy()), &mut sources)
			.map_err(|e| {
				let message = format!("{e}");
				message.lines().next().unwrap_or_default().to_string()
			})?;
	let history = History::build(&loaded.items);
	Ok(match history.latest {
		Some((_, latest)) => format!(
			"a ledger of {}, the latest on {latest}",
			plural(history.entries, "entry")
		),
		None => "a ledger with no entries yet".into(),
	})
}

/// Makes an existing ledger the default, after saying what it is
fn adopt(
	path: &Path,
	request: &InitRequest,
	asking: bool,
	mode: Mode,
) -> Result<Output, Error> {
	let shown = config::display(path);
	let about = describe_ledger(path);
	let mut doc = Doc::new();
	if !asking {
		doc.push(
			Line::styled("◇ ", Style::color(palette::ACCENT))
				.with(shown.clone(), Style::color(palette::ACCENT).bold())
				.with(
					" already exists, so it is left as it is",
					Style::faint(),
				),
		);
	}
	if let Err(problem) = &about
		&& !asking
	{
		return Err(Diagnostic::error(format!(
			"`{shown}` exists, but ledr couldn't read it as a ledger: {problem}"
		))
		.help("choose another place for a new ledger, like `ledr init ~/books`")
		.into());
	}
	if request.dry_run {
		doc.push(Line::faint(if request.remember {
			"Dry run: it wasn't made the default ledger."
		} else {
			"Dry run: nothing was changed."
		}));
		return Ok(Output::text(mode.render(&doc)));
	}
	let remember = match should_remember(path, request, asking) {
		Ok(remember) => remember,
		Err(e) if interactive::is_cancel(&e) => {
			doc.push(Line::faint("Cancelled. Nothing was changed."));
			return Ok(Output::text(mode.render(&doc)));
		},
		Err(e) => return Err(e),
	};
	if remember {
		doc.extend(remembered_lines(path)?);
		if let Ok(about) = about {
			doc.push(Line::faint(format!("  It's {about}.")));
		}
	} else {
		doc.push(Line::faint(
			"Nothing was changed: ledr's default ledger is as it was.",
		));
	}
	doc.blank();
	doc.extend(next_steps(&shown, remember, "ledr add coffee 4.50"));
	Ok(Output::text(mode.render(&doc)))
}

/// Whether to make `path` the default ledger, asking before replacing
/// another
fn should_remember(
	path: &Path,
	request: &InitRequest,
	asking: bool,
) -> Result<bool, Error> {
	if !request.remember {
		return Ok(false);
	}
	let current = config::load(&mut SourceMap::new())
		.ok()
		.flatten()
		.as_ref()
		.and_then(config::ledger)
		.map(PathBuf::from);
	match current {
		Some(current) if asking && current != path && current.exists() => {
			interactive::confirm_with(
				&format!(
					"ledr reads {} by default. Read {} instead?",
					config::display(&current),
					config::display(path)
				),
				true,
				"you can still read either with -f",
			)
		},
		_ => Ok(true),
	}
}

/// Saves `path` as the default ledger, and says so
fn remembered_lines(path: &Path) -> Result<Vec<Line>, Error> {
	let config_path = config::remember(path)?;
	let mut lines = vec![done("ledr will read it by default").with(
		format!(" (see {})", config::display(&config_path)),
		Style::faint(),
	)];
	// The environment wins over the config, which could be a surprise
	if let Ok(env) = std::env::var("LEDR_FILE")
		&& !env.is_empty()
		&& setup::resolve_path(&env) != path
	{
		lines.push(Line::styled("⚠ ", Style::color(palette::AMBER)).with(
			format!(
				"LEDR_FILE is set to {env}, which ledr reads instead. \
						Unset it, or set it to this ledger."
			),
			Style::new(),
		));
	}
	Ok(lines)
}

/// Writes a new file, making its folder if need be, and never replacing
/// anything
fn create_new(path: &Path, text: &str) -> Result<(), Error> {
	use std::io::Write;
	let shown = config::display(path);
	if let Some(dir) = path.parent() {
		std::fs::create_dir_all(dir).map_err(|e| {
			anyhow::anyhow!("Could not create `{}`: {e}", config::display(dir))
		})?;
	}
	let mut file = std::fs::OpenOptions::new()
		.write(true)
		.create_new(true)
		.open(path)
		.map_err(|e| anyhow::anyhow!("Could not create `{shown}`: {e}"))?;
	file.write_all(text.as_bytes())
		.and_then(|_| file.sync_all())
		.map_err(|e| anyhow::anyhow!("Could not write `{shown}`: {e}"))
}

fn done(text: &str) -> Line {
	Line::styled("✓ ", Style::color(palette::GREEN).bold())
		.with(text.to_string(), Style::new())
}

/// The new ledger in brief: its opening entry, what's declared, and what
/// it all comes to
fn preview(setup: &Setup, mode: Mode) -> Doc {
	let mut doc = Doc::new();
	doc.blank();
	doc.push(rule("Your ledger", mode.width().min(72)));
	if let Some(entry) = setup.opening() {
		let text = entry.to_text(
			&crate::tidy::FileStyle::default(),
			&History::build(&[]),
			&setup::decimals,
		);
		for line in render::entries::highlight(&text) {
			doc.push(Line::plain("  ").then(line));
		}
		doc.blank();
	}
	let held = setup
		.accounts
		.iter()
		.filter(|a| a.account.starts_with("Assets"))
		.count();
	let owed = setup.accounts.len() - held;
	let counts: Vec<String> = [
		(held, "account"),
		(owed, "debt"),
		(setup.categories.len(), "category"),
	]
	.into_iter()
	.filter(|(n, _)| *n > 0)
	.map(|(n, what)| plural(n, what))
	.collect();
	doc.push(
		Line::styled("  ✚ ", Style::color(palette::ACCENT))
			.then(dotted(&counts))
			.with(format!(", all opened on {}", setup.date), Style::faint()),
	);
	let worth = setup.net_worth();
	if !worth.is_empty() {
		let mut line = Line::styled("  = ", Style::faint())
			.with("Net worth ", Style::faint());
		for (i, (currency, value)) in worth.iter().enumerate() {
			if i > 0 {
				line.push(" + ", Style::faint());
			}
			let shown =
				compose::amount_text(*value, currency, &setup::decimals);
			line.append(render::money(&shown, currency, value.is_negative()));
		}
		doc.push(line);
	}
	doc.blank();
	doc
}

/// A quick `ledr add` that works on a new ledger, before it has any
/// history to go by: a category named by the description, paid from a card
/// if there is one, like `ledr add groceries 54.20 @visa`. Accounts it
/// doesn't have are named in full, and would be added.
fn first_add(setup: &Setup) -> String {
	let last = |account: &str| {
		account.rsplit(':').next().unwrap_or(account).to_lowercase()
	};
	let category = setup
		.categories
		.iter()
		.find(|c| c.ends_with(":Groceries"))
		.or_else(|| {
			setup.categories.iter().find(|c| c.starts_with("Expenses:"))
		})
		.map_or_else(|| "coffee @Expenses:Coffee".to_string(), |c| last(c));

	let is_card = |account: &str| {
		let name = account.to_lowercase();
		["card", "visa", "mastercard", "amex"]
			.iter()
			.any(|card| name.contains(card))
	};
	let accounts = || setup.accounts.iter().map(|a| a.account.as_str());
	let paid = accounts()
		.find(|a| a.starts_with("Liabilities:") && is_card(a))
		.or_else(|| accounts().find(|a| a.starts_with("Assets:")))
		.map_or_else(|| "Assets:Cash".to_string(), last);

	let amount = if setup::decimals(&setup.currency) == 0 {
		"5420"
	} else {
		"54.20"
	};
	format!("ledr add {category} {amount} @{paid}")
}

/// What to do with a ledger, new or not. `example` is a quick `ledr add`
/// that will work on it. Unless it is the default ledger, every command
/// names it.
fn next_steps(shown: &str, remembered: bool, example: &str) -> Vec<Line> {
	let ledr = if remembered {
		"ledr".to_string()
	} else {
		format!("ledr -f {shown}")
	};
	let example = example.replacen("ledr", &ledr, 1);
	let steps = [
		(format!("{ledr} add"), "record what you spend and earn"),
		(example, "or in a few words"),
		(ledr.clone(), "see where things stand"),
		(format!("{ledr} check"), "look for mistakes"),
	];
	let width = steps
		.iter()
		.map(|(c, _)| c.chars().count())
		.max()
		.unwrap_or(0);

	let mut lines = vec![Line::styled("Next", Style::new().bold())];
	for (command, what) in steps {
		lines.push(
			Line::plain("  ")
				.with(
					format!("{command:<width$}  "),
					Style::color(palette::GREEN),
				)
				.with(what, Style::faint()),
		);
	}
	lines.push(Line::new());
	lines.push(Line::faint(
		"It's plain text, so keep it in git for history and backups.",
	));
	lines
}

/// Formats every file of the ledger, showing or writing the changes
pub fn tidy(
	ctx: &Context,
	sources: &mut SourceMap,
	write: bool,
	check: bool,
) -> Result<Output, Error> {
	loader::load(&ctx.load.file, sources, None)?;

	let mut changed: Vec<(std::path::PathBuf, String, String)> = vec![];
	let mut files = 0;
	for (id, file) in sources.files() {
		if file.path.as_os_str() == "<stdin>" {
			continue;
		}
		files += 1;
		let tidied = crate::tidy::tidy(&file.text, id)
			.map_err(|e| anyhow::anyhow!("{}: {e:#}", file.path.display()))?;
		if tidied != file.text {
			changed.push((file.path.clone(), file.text.clone(), tidied));
		}
	}

	let color = ctx.mode.color();
	let ok = |text: String| {
		Line::styled("✓ ", Style::color(palette::GREEN).bold())
			.with(text, Style::new().bold())
	};
	if changed.is_empty() {
		return Ok(Output::text(
			ok(format!("Already tidy ({})", plural(files, "file")))
				.render(color)
				+ "\n",
		));
	}

	if write {
		for (path, _, tidied) in &changed {
			write_atomically(path, tidied)?;
		}
		return Ok(Output::text(
			ok(format!("Tidied {}", plural(changed.len(), "file")))
				.render(color)
				+ "\n",
		));
	}

	let mut text = String::new();
	if !check {
		for (path, before, after) in &changed {
			text.push_str(&match ctx.mode {
				Mode::Plain => crate::tidy::plain_diff(path, before, after),
				Mode::Fancy { .. } => {
					ctx.mode
						.render(&crate::tidy::fancy_diff(path, before, after))
						+ "\n"
				},
			});
		}
	} else {
		for (path, _, _) in &changed {
			text.push_str(&format!("{}\n", path.display()));
		}
	}
	let summary = Line::styled("◇ ", Style::color(palette::ACCENT))
		.with(
			format!("{} would change", plural(changed.len(), "file")),
			Style::new().bold(),
		)
		.with(" · run ", Style::faint())
		.with("ledr tidy --write", Style::color(palette::ACCENT))
		.with(" to apply", Style::faint());
	Ok(Output {
		text,
		notice: Some(summary.render(color)),
		failed: check,
	})
}

/// Replaces a file's contents without ever leaving it half written: the new
/// text goes to a temporary file beside it, which then takes its place.
/// Symbolic links are followed, so the file they point to is the one
/// changed, and read-only files are left alone.
fn write_atomically(path: &Path, text: &str) -> Result<(), Error> {
	use std::io::Write;

	let real = std::fs::canonicalize(path)?;
	let permissions = std::fs::metadata(&real)?.permissions();
	if permissions.readonly() {
		return Err(Diagnostic::error(format!(
			"`{}` is read-only, so it was left as it is",
			path.display()
		))
		.into());
	}

	let mut temp = real.as_os_str().to_owned();
	temp.push(".ledr-tidy");
	let temp = std::path::PathBuf::from(temp);
	let written = (|| -> std::io::Result<()> {
		let mut file = std::fs::OpenOptions::new()
			.write(true)
			.create_new(true)
			.open(&temp)?;
		file.set_permissions(permissions)?;
		file.write_all(text.as_bytes())?;
		file.sync_all()?;
		std::fs::rename(&temp, &real)
	})();
	if let Err(error) = written {
		let _ = std::fs::remove_file(&temp);
		return Err(anyhow::anyhow!(
			"Could not write `{}`: {error}",
			path.display()
		));
	}
	Ok(())
}

/// Every account, with how much it is used
pub fn accounts(
	ctx: &Context,
	sources: &mut SourceMap,
) -> Result<Output, Error> {
	let items = loader::load(&ctx.load.file, sources, None)?;
	let history = History::build(&items);
	let all = history.all_accounts();

	Ok(Output::text(match ctx.mode {
		Mode::Plain => all.iter().map(|a| format!("{a}\n")).collect(),
		Mode::Fancy { width, .. } => {
			let most = history
				.accounts
				.values()
				.map(|u| u.count)
				.max()
				.unwrap_or(1);
			let mut table = Table::new(vec![
				Column::left("Account").shrinking(),
				Column::right("Uses"),
				Column::left("Last used"),
				Column::left(""),
			]);
			for account in &all {
				let (uses, last, bar) = match history.accounts.get(*account) {
					Some(u) => (
						Line::plain(u.count.to_string()),
						Line::faint(u.last.to_string()),
						meter(
							u.count as f64 / most as f64,
							16,
							render::category_color(account),
						),
					),
					None => {
						(Line::faint("0"), Line::faint("never"), Line::new())
					},
				};
				table.row(vec![render::account(account), uses, last, bar]);
			}
			let mut doc = Doc::new();
			doc.push(rule(&plural(all.len(), "account"), width));
			doc.extend(table.render(width));
			ctx.mode.render(&doc)
		},
	}))
}

/// Every description used, most used first
pub fn payees(ctx: &Context, sources: &mut SourceMap) -> Result<Output, Error> {
	let items = loader::load(&ctx.load.file, sources, None)?;
	let history = History::build(&items);

	Ok(Output::text(match ctx.mode {
		Mode::Plain => {
			history.payees.keys().map(|p| format!("{p}\n")).collect()
		},
		Mode::Fancy { width, .. } => {
			let mut payees: Vec<_> = history.payees.values().collect();
			payees.sort_by(|a, b| {
				b.usage.count.cmp(&a.usage.count).then(a.name.cmp(&b.name))
			});
			let most = payees.first().map_or(1, |p| p.usage.count);
			let mut table = Table::new(vec![
				Column::left("Description").shrinking(),
				Column::right("Uses"),
				Column::left("Last used"),
				Column::left("Usually").shrinking(),
				Column::left(""),
			]);
			for payee in &payees {
				let usual = payee
					.template
					.first()
					.map(|p| p.account.as_str())
					.unwrap_or("");
				table.row(vec![
					Line::plain(payee.name.clone()),
					Line::plain(payee.usage.count.to_string()),
					Line::faint(payee.usage.last.to_string()),
					render::account(usual),
					meter(
						payee.usage.count as f64 / most as f64,
						12,
						palette::ACCENT,
					),
				]);
			}
			let mut doc = Doc::new();
			doc.push(rule(&plural(payees.len(), "description"), width));
			doc.extend(table.render(width));
			ctx.mode.render(&doc)
		},
	}))
}

/// An at-a-glance summary: net worth, recent months, where the money went,
/// and the latest entries. What `ledr` shows with no command.
pub fn overview(
	ctx: &Context,
	sources: &mut SourceMap,
) -> Result<Output, Error> {
	let mut loaded = ctx.load(sources)?;
	let history = History::build(&loaded.items);
	let main = match history.main_currency() {
		Some(main) if history.entries > 0 => main.to_string(),
		_ => {
			return Ok(Output::text(
				"The ledger has no entries yet. Add one with `ledr add`.\n"
					.into(),
			));
		},
	};
	let precision = loaded
		.result
		.max_precision_by_currency
		.get(&main)
		.copied()
		.unwrap_or(crate::util::amount::DEFAULT_PRECISION);
	let show = |q: Quant| {
		let mut q = q;
		q.round(precision);
		q.to_string()
	};

	// The month to focus on: this one, unless the ledger stops earlier
	let latest = loaded
		.ledger
		.entries()
		.last()
		.map_or(ctx.today, |e| *e.get_date());
	let anchor = latest.min(ctx.today);
	let (month_start, month_end) =
		parse_period(&format!("{}-{}", anchor.year(), anchor.month()), anchor)?;
	let (last_start, last_end) = parse_period("last-month", anchor)?;
	let month_name = |d: Date| d.to_naive().format("%B %Y").to_string();

	// Balances in the main currency, at the latest known rates
	let totals_between =
		|ledger: &crate::gl::ledger::Ledger, from: Date, to: Date| {
			let details: Vec<_> = ledger
				.entries()
				.iter()
				.filter(|e| *e.get_date() >= from && *e.get_date() <= to)
				.flat_map(|e| e.details().iter().cloned())
				.collect();
			let mut total = Total::new();
			total.ingest_details(&details);
			total
		};
	let in_main = |total: &Total, category: &str| -> Quant {
		total
			.subtotals
			.get(category)
			.and_then(|t| t.amounts().get(&main).copied())
			.unwrap_or_default()
	};

	let mut all = totals_between(&loaded.ledger, Date::min(), Date::max());
	all.collapse_to(&main, &mut loaded.ledger.exchange_rates, false);
	let net_worth = in_main(&all, "Assets") + in_main(&all, "Liabilities");

	let mut months = vec![];
	let mut spending: Vec<(String, Quant)> = vec![];
	for (i, (from, to)) in [(month_start, month_end), (last_start, last_end)]
		.into_iter()
		.enumerate()
	{
		let mut total = totals_between(&loaded.ledger, from, to);
		total.collapse_to(&main, &mut loaded.ledger.exchange_rates, true);
		let earned = -in_main(&total, "Income");
		let spent = in_main(&total, "Expenses");
		months.push((month_name(from), earned, spent));
		if i == 0
			&& let Some(expenses) = total.subtotals.get("Expenses")
		{
			for (name, sub) in &expenses.subtotals {
				let value =
					sub.amounts().get(&main).copied().unwrap_or_default();
				if !value.is_zero() {
					spending.push((name.clone(), value));
				}
			}
		}
	}
	spending.sort_by_key(|(_, value)| std::cmp::Reverse(*value));

	let recent: Vec<&Entry> =
		loaded.ledger.entries().iter().rev().take(5).collect();
	let warnings = loaded.ledger.warnings.len();

	if !ctx.mode.is_fancy() {
		let mut out = format!("Net worth: {} {main}\n", show(net_worth));
		for (name, earned, spent) in &months {
			out.push_str(&format!(
				"{name}: earned {} {main}, spent {} {main}\n",
				show(*earned),
				show(*spent)
			));
		}
		return Ok(Output::text(out));
	}

	let width = ctx.mode.width();
	let (first, last) = ctx.span_of(&loaded);
	let mut body = vec![
		badge("LEDR", palette::ACCENT)
			.with("  ", Style::new())
			.then(dotted(&[
				ctx.file_name(),
				plural(loaded.ledger.entries().len(), "entry"),
				format!("{first} → {last}"),
			])),
		Line::new(),
	];
	let label_width =
		months.iter().map(|m| m.0.len()).max().unwrap_or(0).max(9) + 3;
	let numbers: Vec<String> = std::iter::once(show(net_worth))
		.chain(months.iter().map(|(_, e, s)| show((*e - *s).abs())))
		.collect();
	let decimals = Decimals::fit(numbers.iter().map(String::as_str));
	body.push(
		Line::plain(format!("{:<label_width$}", "Net worth"))
			.with(
				format!("  {}", decimals.align(&numbers[0])),
				Style::color(gain_color(&net_worth)).bold(),
			)
			.with(format!(" {main}"), Style::faint()),
	);
	for ((name, earned, spent), number) in months.iter().zip(&numbers[1..]) {
		let saved = *earned - *spent;
		let color = gain_color(&saved);
		let (arrow, verb) = if saved.is_negative() {
			("▼", "overspent")
		} else {
			("▲", "saved")
		};
		let mut line = Line::plain(format!("{name:<label_width$}"))
			.with(format!("{arrow} "), Style::color(color))
			.with(decimals.align(number), Style::color(color).bold())
			.with(format!(" {main} {verb}"), Style::faint());
		if earned.is_negative() || earned.is_zero() {
			line.push("  no income", Style::faint());
		} else {
			line.push(
				format!("   {} of income", percent((saved / *earned).to_f64())),
				Style::faint(),
			);
		}
		body.push(line);
	}

	let mut doc = Doc::new();
	doc.extend(panel(body, palette::ACCENT, None, width));

	if !spending.is_empty() {
		doc.blank();
		doc.push(rule(&format!("Spending in {}", months[0].0), width));
		let total: Quant = spending.iter().map(|(_, v)| *v).sum();
		let largest = spending[0].1.to_f64();
		let numbers: Vec<String> =
			spending.iter().map(|(_, v)| show(*v)).collect();
		let decimals = Decimals::fit(numbers.iter().map(String::as_str));
		let name_width =
			spending.iter().map(|(n, _)| n.len()).max().unwrap_or(0);
		for ((name, value), number) in spending.iter().take(8).zip(&numbers) {
			doc.push(
				Line::plain(format!("{name:<name_width$}  "))
					.with(decimals.align(number), Style::new())
					.with(format!(" {main}  "), Style::faint())
					.then(meter(value.to_f64() / largest, 24, palette::AMBER))
					.with(
						format!(" {:>4}", percent((*value / total).to_f64())),
						Style::faint(),
					),
			);
		}
	}

	if !recent.is_empty() {
		doc.blank();
		doc.push(rule("Recent entries", width));
		let mut table = Table::new(vec![
			Column::left(""),
			Column::left("").shrinking(),
			Column::left("").shrinking(),
			Column::right(""),
		]);
		for entry in &recent {
			let first = entry
				.details()
				.iter()
				.find(|d| !d.is_system())
				.or(entry.details().first());
			let (account, amount) = match first {
				Some(d) => (
					render::account(d.account()),
					render::money(
						&d.value().to_string(),
						d.currency(),
						d.value().is_negative(),
					),
				),
				None => (Line::new(), Line::new()),
			};
			table.row(vec![
				Line::faint(entry.get_date().to_string()),
				Line::plain(entry.get_desc().clone()),
				account,
				amount,
			]);
		}
		doc.extend(table.render(width));
	}

	doc.blank();
	doc.push(if warnings == 0 {
		Line::styled("✓ ", Style::color(palette::GREEN))
			.with("No warnings", Style::faint())
	} else {
		Line::styled("⚠ ", Style::color(palette::AMBER))
			.with(plural(warnings, "warning"), Style::faint())
			.with(" · run ", Style::faint())
			.with("ledr check", Style::color(palette::ACCENT))
	});
	doc.push(
		Line::faint("Try ")
			.with("ledr bs", Style::color(palette::ACCENT))
			.with(" · ", Style::faint())
			.with("ledr is -P last-month", Style::color(palette::ACCENT))
			.with(" · ", Style::faint())
			.with("ledr add", Style::color(palette::ACCENT))
			.with(" · ", Style::faint())
			.with("ledr --help", Style::color(palette::ACCENT)),
	);
	Ok(Output::text(ctx.mode.render(&doc)))
}
