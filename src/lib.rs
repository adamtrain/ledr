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

//! Ledr: a plain text accounting tool.
//!
//! The pipeline runs in one direction:
//!
//! 1. [`syntax`] lexes lines of ledger text into typed parts, and
//!    [`parsing::loader`] reads a file and its includes into those lines.
//! 2. [`parsing::parser`] builds a [`gl::ledger::Ledger`] from them, which
//!    balances entries, tracks exchange rates in [`gl::exchange_rates`] and
//!    lot activity in [`investment`].
//! 3. [`reports`] turns the finished ledger into report models, which
//!    [`render`] draws either plainly or with color and charts.
//!
//! [`input`] and [`tidy`] go the other way, helping to write ledger text.

pub mod commands;
pub mod diagnostics;
pub mod gl;
pub mod input;
pub mod investment;
pub mod parsing;
pub mod render;
pub mod reports;
pub mod syntax;
pub mod tidy;
pub mod ui;
pub mod util;
