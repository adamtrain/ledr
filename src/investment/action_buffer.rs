/* Copyright © 2024-2025 Adam Train <adam@trainrelay.net>
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

use crate::investment::action::{Action, Direction};
use crate::investment::portfolio::Portfolio;
use anyhow::Error;
use std::cmp::Ordering;

/// Stores actions related to lots until they are all in, at which time this
/// sorts them and assembles a Portfolio for inspection.
#[derive(Debug, Default)]
pub struct ActionBuffer {
	actions: Vec<Action>, // all actions, unordered
}

impl ActionBuffer {
	/// Adds an action to the lot buffer.
	pub fn add_action(&mut self, action: Action) {
		self.actions.push(action);
	}

	/// Aggregates all actions into lots, in chronological order by date.
	/// On the same date, all buys come before all sells, to account for
	/// order of appearance not being guaranteed. Fails if a Sell action
	/// has no corresponding lot from which to sell, else succeeds and
	/// results in a portfolio.
	pub fn tabulate(&mut self) -> Result<Portfolio, Error> {
		self.sort_actions();

		let mut state = Portfolio::new();

		for action in &mut self.actions {
			match &action.direction {
				Direction::Buy => {
					state.buy_lot(action);
				},
				Direction::Sell(proceeds) => {
					state.sell_lot(action, proceeds.clone())?;
				},
			}
		}
		Ok(state)
	}

	fn sort_actions(&mut self) {
		self.actions
			.sort_by(|a, b| a.partial_cmp(b).unwrap_or(Ordering::Equal));
	}
}
