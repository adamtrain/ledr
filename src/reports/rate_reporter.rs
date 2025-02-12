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
use crate::gl::observed_rate::{ObservationType, ObservedRate};
use crate::reports::table::Table;
use std::collections::BTreeMap;

pub struct RateReporter {
	rates: BTreeMap<(String, String), Vec<ObservedRate>>,
}

impl RateReporter {
	pub fn new(
		mut rates: BTreeMap<(String, String), Vec<ObservedRate>>,
	) -> RateReporter {
		for rate_set in rates.values_mut() {
			for rate in rate_set.iter_mut() {
				rate.make_visible();
			}
		}

		Self { rates }
	}

	pub fn print_all_rates(&self) {
		let mut table = Table::new(5);

		table.add_header(vec!["Base", "Quote", "Observed", "Rate", "T"]);
		table.add_separator();

		for ((base, quote), rate_set) in &self.rates {
			// Hacky way of checking whether the given rate was an indirect,
			// inferred rate or a rate actually observed between currencies.
			for observation in rate_set {
				let reported_date = if observation.date.is_none() {
					"Multiple".to_string()
				} else {
					observation.date.unwrap().to_string()
				};

				table.add_row(vec![
					base,
					quote,
					&reported_date,
					&observation.rate.to_string(),
					match observation.observation_type {
						ObservationType::Declared => "De",
						ObservationType::Direct => "Ob",
						ObservationType::Inferred => "In",
					},
				])
			}
		}

		table.print();
	}
}
