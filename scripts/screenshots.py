# /// script
# requires-python = ">=3.10"
# dependencies = ["rich>=13"]
# ///
"""Regenerate the README screenshots in docs/.

    make screenshots        # or: cargo build --release && uv run scripts/screenshots.py

Each one runs the real ledr binary, with fancy output forced on, against the
made-up ledger in examples/demo.ledr, and saves what it prints as an SVG.
"""

from __future__ import annotations

import io
import os
import subprocess
import tempfile
from pathlib import Path

from rich.console import Console
from rich.terminal_theme import TerminalTheme
from rich.text import Text

ROOT = Path(__file__).resolve().parent.parent
DOCS = ROOT / "docs"
DEMO = ROOT / "examples" / "demo.ledr"
PROMPT = "❯ "  # a shell-prompt chevron

# The same dark theme as panc's screenshots, so the two tools look related
THEME = TerminalTheme(
    background=(16, 18, 25),
    foreground=(226, 228, 236),
    normal=[
        (32, 34, 44),
        (242, 80, 110),
        (31, 191, 143),
        (235, 154, 18),
        (124, 131, 247),
        (168, 113, 247),
        (86, 182, 194),
        (200, 202, 212),
    ],
    bright=[
        (92, 96, 112),
        (255, 110, 136),
        (70, 214, 170),
        (250, 184, 60),
        (152, 158, 255),
        (190, 146, 255),
        (120, 208, 220),
        (255, 255, 255),
    ],
)

# A ledger with mistakes in it, to show what `ledr check` says
BROKEN = """\
! 2026-01-01 currency USD
! 2026-01-01 account Assets:Checking
! 2026-01-01 account Expenses:Food

2026-01-02 Groceries
    Expenses:Food     42.10 USD
    Assets:Chekcing

2026-01-04 Dinner
    Expenses:Dining   30.00 USD
    Assets:Checking  -29.00 USD
"""


def ledr() -> Path:
    for build in ("release", "debug"):
        binary = ROOT / "target" / build / "ledr"
        if binary.exists():
            return binary
    raise SystemExit("build ledr first: cargo build --release")


def run(args: list[str], width: int, file: Path) -> str:
    # Run beside the ledger, so paths in messages are short

    env = {
        **os.environ,
        "COLORTERM": "truecolor",
        "COLUMNS": str(width),
        "LEDR_FILE": file.name,
    }
    env.pop("NO_COLOR", None)
    result = subprocess.run(
        [str(ledr()), "--fancy", *args],
        env=env,
        cwd=file.parent,
        capture_output=True,
        text=True,
        check=False,
    )
    return result.stdout + result.stderr


def shot(name: str, args: list[str], width: int = 96, file: Path = DEMO) -> None:
    console = Console(
        record=True,
        width=width,
        force_terminal=True,
        color_system="truecolor",
        highlight=False,
        file=io.StringIO(),
    )
    command = " ".join(["ledr", *(f'"{a}"' if " " in a else a for a in args)])
    console.print(Text.assemble((PROMPT, "bold #7c83f7"), (command, "bold")))
    console.print(Text.from_ansi(run(args, width, file).rstrip("\n")), soft_wrap=True)
    console.save_svg(str(DOCS / f"{name}.svg"), title="ledr", theme=THEME)
    print(f"docs/{name}.svg")


def main() -> None:
    DOCS.mkdir(exist_ok=True)
    shot("overview", [])
    shot("income", ["is", "-P", "2026-08", "-i"])
    shot("add", ["add", "2026-09-21", "coffee", "4.75", "--dry-run"])
    shot("unrealized", ["ugl", "-e", "2026-09-20"])
    with tempfile.TemporaryDirectory() as tmp:
        broken = Path(tmp) / "books.ledr"
        broken.write_text(BROKEN)
        shot("check", ["check"], file=broken)


if __name__ == "__main__":
    main()
