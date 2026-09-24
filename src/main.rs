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

mod cli;

use anyhow::{Error, anyhow};
use clap::{CommandFactory, Parser};
use cli::{Cli, Command, Global};
use ledr::commands::{self, Context, Output};
use ledr::diagnostics::Diagnostic;
use ledr::parsing::LoadOptions;
use ledr::render::statement::StatementKind;
use ledr::render::{self, Mode};
use ledr::syntax::source::SourceMap;
use ledr::ui::style::{self, ColorLevel};
use ledr::util::date::Date;
use ledr::util::period::{Edge, parse_bound, parse_period};
use ledr::util::quant::OVERFLOW_MESSAGE;
use std::io::{IsTerminal, Write};
use std::process::ExitCode;

fn main() -> ExitCode {
	install_panic_hook();
	let cli = Cli::parse();

	let mode = output_mode(&cli.global, std::io::stdout().is_terminal());
	let error_mode = output_mode(&cli.global, std::io::stderr().is_terminal());

	let mut sources = SourceMap::new();
	match run(cli, mode, &mut sources) {
		Ok(output) => {
			write_stdout(&output.text);
			if let Some(notice) = output.notice {
				eprintln!("{notice}");
			}
			if output.failed {
				ExitCode::FAILURE
			} else {
				ExitCode::SUCCESS
			}
		},
		Err(error) => {
			eprint!(
				"{}",
				render::diagnostics::error(&error, &sources, error_mode)
			);
			ExitCode::FAILURE
		},
	}
}

/// Fancy in a terminal, plain otherwise, unless told. Setting LEDR_PLAIN
/// makes plain the default everywhere.
fn output_mode(global: &Global, is_terminal: bool) -> Mode {
	let prefer_plain = std::env::var("LEDR_PLAIN").is_ok_and(|v| {
		!matches!(v.to_lowercase().as_str(), "" | "0" | "false" | "no")
	});
	if global.plain || (!global.fancy && (!is_terminal || prefer_plain)) {
		Mode::Plain
	} else {
		Mode::Fancy {
			color: ColorLevel::detect(),
			width: style::terminal_width(),
		}
	}
}

fn run(cli: Cli, mode: Mode, sources: &mut SourceMap) -> Result<Output, Error> {
	let Some(command) = cli.command else {
		// With a ledger to look at, show an overview; otherwise, help
		if cli.global.file.is_some() {
			let ctx = context(&cli.global, mode)?;
			return commands::overview(&ctx, sources);
		}
		Cli::command().print_help()?;
		return Ok(plain_output(String::new()));
	};

	if let Command::Completions { shell } = command {
		let mut out = vec![];
		clap_complete::generate(shell, &mut Cli::command(), "ledr", &mut out);
		return Ok(plain_output(String::from_utf8_lossy(&out).into_owned()));
	}

	let ctx = context(&cli.global, mode)?;
	match command {
		Command::Bs => {
			commands::statement(&ctx, sources, StatementKind::Balance)
		},
		Command::Is => {
			commands::statement(&ctx, sources, StatementKind::Income)
		},
		Command::Tb => commands::statement(&ctx, sources, StatementKind::Trial),
		Command::Er => commands::rates(&ctx, sources),
		Command::Rgl => commands::realized(&ctx, sources),
		Command::Ugl => commands::unrealized(&ctx, sources),
		Command::As { account } => commands::account(&ctx, sources, &account),
		Command::Fmt => commands::print(&ctx, sources, None),
		Command::Find { term } => commands::print(&ctx, sources, Some(&term)),
		Command::Check { strict } => commands::check(&ctx, sources, strict),
		Command::Add(args) => {
			let request = commands::AddRequest {
				words: args.words,
				to: args.to,
				yes: args.yes,
				dry_run: args.dry_run,
				interactive: std::io::stdin().is_terminal()
					&& std::io::stdout().is_terminal(),
			};
			commands::add(&ctx, sources, &request)
		},
		Command::Tidy(args) => {
			commands::tidy(&ctx, sources, args.write, args.check)
		},
		Command::Accounts => commands::accounts(&ctx, sources),
		Command::Payees => commands::payees(&ctx, sources),
		Command::Completions { .. } => unreachable!(),
	}
}

fn plain_output(text: String) -> Output {
	Output {
		text,
		notice: None,
		failed: false,
	}
}

fn context(global: &Global, mode: Mode) -> Result<Context, Error> {
	let file = global.file.clone().ok_or_else(|| {
		Diagnostic::error("No ledger file given")
			.help("pass one with -f FILE, or set LEDR_FILE in your environment")
	})?;

	let today = Date::today();
	let date = |text: &Option<String>,
	            edge: Edge,
	            flag: &str|
	 -> Result<Option<Date>, Error> {
		text.as_deref()
			.map(|t| {
				parse_bound(t, edge, today).map_err(|e| anyhow!("{flag}: {e}"))
			})
			.transpose()
	};
	let (mut begin, mut end) = (
		date(&global.begin, Edge::Start, "--begin")?,
		date(&global.end, Edge::End, "--end")?,
	);
	if let Some(period) = &global.period {
		let (b, e) = parse_period(period, today).map_err(|_| {
			anyhow!(
				"--period: `{period}` is not a period I understand; try 2024, \
				2024-03, 2024-Q1, last-month or ytd"
			)
		})?;
		begin = Some(b);
		end = Some(e);
	}

	let mut load = LoadOptions::new(file);
	load.lenient = global.lenient;
	load.begin = begin.unwrap_or_else(Date::min);
	load.end = end.unwrap_or_else(Date::max);
	load.precision = global.precision;

	Ok(Context {
		load,
		mode,
		currency: global.currency.clone(),
		ignore_other_currencies: global.ignore_other_currencies,
		ignore_equity: global.ignore_equity,
		depth: global.depth,
		invert: global.invert,
		begin_given: begin.is_some(),
		end_given: end.is_some(),
		today,
	})
}

/// Writes to stdout, quietly stopping if the reader has gone away, as when
/// piping into `head`
fn write_stdout(text: &str) {
	let mut stdout = std::io::stdout().lock();
	if stdout
		.write_all(text.as_bytes())
		.and_then(|_| stdout.flush())
		.is_err()
	{
		// Nothing useful to do; the reader closed the pipe
	}
}

/// Replaces Rust's panic output with something a person can act on. Every
/// arithmetic overflow panics with a known message, which becomes a normal
/// error; anything else is a bug worth reporting.
fn install_panic_hook() {
	std::panic::set_hook(Box::new(|info| {
		let message = info
			.payload()
			.downcast_ref::<String>()
			.map(String::as_str)
			.or_else(|| info.payload().downcast_ref::<&str>().copied())
			.unwrap_or("unknown error");

		let diagnostic = if message == OVERFLOW_MESSAGE {
			Diagnostic::error(
				"A number is too large or too precise to compute exactly",
			)
			.note("ledr never rounds silently, so it stopped rather than guess")
			.help(
				"look for amounts with very many decimal places, or extreme \
					exchange rates",
			)
		} else {
			let location = info
				.location()
				.map(|l| format!(" at {}:{}", l.file(), l.line()))
				.unwrap_or_default();
			Diagnostic::error(format!("ledr hit a bug: {message}{location}"))
				.help(
					"please report this at https://github.com/adamtrain/ledr/issues",
				)
		};

		let mode = if std::io::stderr().is_terminal() {
			Mode::Fancy {
				color: ColorLevel::detect(),
				width: 80,
			}
		} else {
			Mode::Plain
		};
		let text = match mode {
			Mode::Plain => {
				render::diagnostics::plain(&diagnostic, &SourceMap::new())
			},
			Mode::Fancy { .. } => mode.render(&render::diagnostics::fancy(
				&diagnostic,
				&SourceMap::new(),
			)),
		};
		eprint!("{text}");
	}));
}
