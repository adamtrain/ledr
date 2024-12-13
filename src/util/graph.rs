/* Copyright © 2024-2025 Adam Train <adam@adamtrain.net>
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
use crate::util::amount::Amount;
use crate::util::date::Date;
use crate::util::quant::Quant;
use anyhow::{bail, Error};
use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};

#[derive(Debug)]
pub struct Graph {
	nodes: BTreeMap<String, Node>, // currency symbol -> its Node
	date: Option<Date>,            // reference date for data within
}

#[derive(Debug)]
struct Node {
	edges: BTreeMap<String, Rate>, // currency symbol -> conversion rate
}

#[derive(Debug)]
struct Rate {
	quant: Vec<Quant>,

	observation_type: ObservationType,
	/// There is only one observation date for potentially many observations
	/// because we only accept the observations from the newest date on those
	/// graphs that span multiple days.
	observation_date: Date,
}

impl Rate {
	/// Reports a single rate to the caller, based on all underlying rate data.
	/// Uses only the latest rates by observation date.
	fn avg(&self) -> Quant {
		let latest_date = self
			.quant
			.iter()
			.zip(std::iter::repeat(self.observation_date))
			.max_by_key(|&(_, date)| date)
			.map(|(rate, _)| rate);

		match latest_date {
			Some(rate) => *rate,
			None => unreachable!(),
		}
	}
}

impl Graph {
	pub fn new(date: Date) -> Self {
		Self {
			nodes: Default::default(),
			date: Some(date),
		}
	}

	pub fn new_undated() -> Self {
		Self {
			nodes: Default::default(),
			date: None,
		}
	}

	/// Adds a bidirectional exchange rate between two currencies
	pub fn add_rate(
		&mut self,
		date: &Date,
		a: &Amount,
		b: &Amount,
		observation_type: ObservationType,
	) -> Result<(), Error> {
		let rate_ab = a.value / b.value;
		let rate_ba = b.value / a.value;

		if rate_ab == Quant::zero() || rate_ba == Quant::zero() {
			bail!("Exchange rates cannot be zero");
		}

		self.add_rate_to_node(
			date,
			&a.currency,
			&b.currency,
			rate_ab,
			observation_type,
		);
		self.add_rate_to_node(
			date,
			&b.currency,
			&a.currency,
			rate_ba,
			observation_type,
		);

		Ok(())
	}

	/// Overwrites any existing rate between the currencies, leaving only this
	/// one, the most recent entry.
	pub fn overwrite_rate_if_newer(
		&mut self,
		date: &Date,
		a: &Amount,
		b: &Amount,
		observation_type: ObservationType,
	) -> Result<(), Error> {
		if let Some(existing_date) =
			self.get_date_for_rate(&a.currency, &b.currency)
		{
			if &existing_date < date {
				self.remove_rate(&a.currency, &b.currency);
				self.add_rate(date, a, b, observation_type)?;
			}
		}

		Ok(())
	}

	/// Adds an edge on the node_currency reflecting the given rate to the
	/// edge_currency, with metadata on the edge.
	fn add_rate_to_node(
		&mut self,
		date: &Date,
		node_currency: &str,
		edge_currency: &str,
		rate: Quant,
		observation_type: ObservationType,
	) {
		self.nodes
			.entry(node_currency.to_owned())
			.or_insert_with(Node::new)
			.edges
			.entry(edge_currency.to_owned())
			.or_insert_with(|| Rate {
				quant: vec![],
				observation_type,
				observation_date: *date,
			})
			.quant
			.push(rate);
	}

	/// Gets the date at which a direct rate was observed, if any exists.
	/// Does not traverse.
	pub fn get_date_for_rate(&self, a: &String, b: &String) -> Option<Date> {
		// we only need to check one as they are all symmetric
		if let Some(node) = self.nodes.get(a) {
			if let Some(rate) = node.edges.get(b) {
				return Some(rate.observation_date);
			}
		}
		None
	}

	/// Removes the given currency node and all edges to it.
	pub fn remove_currency(&mut self, currency: &String) {
		if let Some(node) = self.nodes.get_mut(currency) {
			node.edges.remove(currency);
		}

		self.nodes.remove(currency);
	}

	/// Removes edges between the given pair, if any exist, else no-op.
	fn remove_rate(&mut self, a: &String, b: &String) {
		self.nodes.get_mut(a).and_then(|node| node.edges.remove(b));
		self.nodes.get_mut(b).and_then(|node| node.edges.remove(a));
	}

	/// Returns true iff this has any cycles in the graph that do not multiply
	/// out to approximately 1, i.e. if the rates are internally inconsistent.
	///
	/// Costly on performance if the graph is large.
	pub fn has_inconsistent_cycle(&self) -> bool {
		for currency in self.nodes.keys() {
			let mut visited = HashSet::new();
			let mut rec_stack = HashSet::new();
			if self.detect_inconsistent_cycle(
				currency,
				&mut visited,
				&mut rec_stack,
				Quant::from_i128(1),
				currency,
			) {
				return true;
			}
		}

		false
	}

	fn detect_inconsistent_cycle(
		&self,
		current: &str,
		visited: &mut HashSet<String>,
		rec_stack: &mut HashSet<String>,
		rate_product: Quant,
		start: &str,
	) -> bool {
		if rec_stack.contains(current) {
			// Cycle detected, but only check if we're at the start node
			if current == start {
				return rate_product > Quant::from_frac(21, 20)
					|| rate_product < Quant::from_frac(19, 20);
			} else {
				false
			};
		}

		if visited.contains(current) {
			return false;
		}

		// Mark the current node as visited and add to the recursion stack
		visited.insert(current.to_string());
		rec_stack.insert(current.to_string());

		if let Some(node) = self.nodes.get(current) {
			for (neighbor, rate) in &node.edges {
				let new_rate_product = rate_product * rate.avg();

				if self.detect_inconsistent_cycle(
					neighbor,
					visited,
					rec_stack,
					new_rate_product,
					start,
				) {
					return true;
				}
			}
		}

		// Backtrack: Remove from recursion stack
		rec_stack.remove(current);

		false
	}

	/// Reports the average rate between two currencies using all shortest
	/// paths with the highest number of possible declared rates along the
	/// way, as well as the number of hops it took to convert the rate
	/// (adjacent = 1). Returns None if no path exists between the currencies.
	///
	/// The returned bool represents whether the rate was marked as declared
	/// across every step in its traversal, i.e. only computed using declared
	/// rates.
	pub fn convert(
		&self,
		base: &str,
		quote: &str,
	) -> Option<(Quant, u32, bool)> {
		if base == quote {
			return Some((Quant::from_frac(1, 1), 0, true));
		}

		let mut visited = HashMap::new();
		let mut queue = VecDeque::new();
		let mut shortest_path_length = None;
		let mut rates = vec![];

		// (currency, rate, depth, number_of_declared_rates_along_path)
		queue.push_back((quote.to_string(), Quant::from_i128(1), 0, 0));

		while let Some((
			current_currency,
			current_rate,
			depth,
			declared_count,
		)) = queue.pop_front()
		{
			if let Some(node) = self.nodes.get(&current_currency) {
				for (neighbor, rate) in &node.edges {
					let new_rate = current_rate * rate.avg();
					let new_declared_count =
						if rate.observation_type == ObservationType::Declared {
							declared_count + 1
						} else {
							declared_count
						};

					if neighbor == base {
						match shortest_path_length {
							Some(path_len) if depth + 1 < path_len => {
								shortest_path_length = Some(depth + 1);
								rates.clear();
								rates.push((
									new_rate,
									new_declared_count,
									depth + 1,
								));
							},
							Some(path_len) if depth + 1 == path_len => {
								rates.push((
									new_rate,
									new_declared_count,
									depth + 1,
								));
							},
							None => {
								shortest_path_length = Some(depth + 1);
								rates.push((
									new_rate,
									new_declared_count,
									depth + 1,
								));
							},
							_ => {},
						}
					}

					if !visited.contains_key(neighbor)
						|| visited[neighbor] > depth + 1
					{
						visited.insert(neighbor.clone(), depth + 1);
						queue.push_back((
							neighbor.clone(),
							new_rate,
							depth + 1,
							new_declared_count,
						));
					}
				}
			}
		}

		if rates.is_empty() {
			return None;
		}

		// First, try to use the average of paths with the greatest number
		// of declared rates. Basically, ignore paths that inferred more
		// than necessary.
		let max_declared_count =
			rates.iter().map(|&(_, count, _)| count).max().unwrap_or(0);

		// average all rates from the set with the highest declared rate count
		let (total_rate, count) = rates
			.iter()
			.filter(|&&(_, count, _)| count == max_declared_count)
			.fold(
				(Quant::zero(), 0),
				|(acc_rate, acc_count), &(rate, _, _)| {
					(acc_rate + rate, acc_count + 1)
				},
			);

		Some((
			total_rate / count,
			shortest_path_length.unwrap(),
			max_declared_count > 0 && max_declared_count == rates[0].2, // (depth)
		))
	}

	/// Reports whether two currency nodes are adjacent. If they are, it will still report
	/// false if must_be_declared is true and the given rate is not declared.
	pub fn get_direct_rate(
		&self,
		base: &str,
		quote: &str,
		must_be_declared: bool,
	) -> Option<Quant> {
		if base == quote {
			return Some(Quant::from_i128(1));
		}

		let node = match self.nodes.get(quote) {
			Some(node) => node,
			None => return None,
		};

		let rate = match node.edges.get(base) {
			Some(rate) => rate,
			None => return None,
		};

		if must_be_declared
			&& rate.observation_type != ObservationType::Declared
		{
			return None;
		};

		Some(rate.avg())
	}

	pub fn get_all_rates(&self) -> Vec<(String, String, ObservedRate)> {
		let mut rates = Vec::new();

		let mut currencies: Vec<_> = self.nodes.keys().cloned().collect();
		currencies.sort(); // Ensure deterministic order

		for (i, base) in currencies.iter().enumerate() {
			for quote in currencies.iter().skip(i + 1) {
				if let Some((rate, path_len, all_declared)) =
					self.convert(base, quote)
				{
					let observation_type = if all_declared {
						ObservationType::Declared
					} else if path_len == 1 {
						ObservationType::Direct
					} else {
						ObservationType::Inferred
					};

					rates.push((
						base.clone(),
						quote.clone(),
						ObservedRate::new(rate, self.date, observation_type),
					));
					rates.push((
						quote.clone(),
						base.clone(),
						ObservedRate::new(
							rate.recip(),
							self.date,
							observation_type,
						),
					));
				}
			}
		}

		rates
	}
}

impl Node {
	fn new() -> Self {
		Node {
			edges: BTreeMap::new(),
		}
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::util::quant::Quant;

	#[test]
	fn test_direct_conversion() {
		let mut graph: Graph = Graph::new_undated();
		let a = Amount::new(Quant::new(2, 0), "USD");
		let b = Amount::new(Quant::new(1, 0), "EUR");
		graph
			.add_rate(
				&Date::from_str("2024-11-12").unwrap(),
				&a,
				&b,
				ObservationType::Inferred,
			)
			.expect("Could not add rate");

		let expected_output = Quant::new(5, 1);
		let result = graph.convert("USD", "EUR");
		assert!(result.is_some());

		let (out, path_len, _) = result.unwrap();

		assert_eq!(expected_output, out);
		assert_eq!(path_len, 1);
		assert!(!graph.has_inconsistent_cycle());
	}

	#[test]
	fn test_reverse_conversion() {
		let mut graph: Graph = Graph::new_undated();
		let a = Amount::new(Quant::new(2, 0), "USD");
		let b = Amount::new(Quant::new(1, 0), "EUR");
		graph
			.add_rate(
				&Date::from_str("2024-11-12").unwrap(),
				&a,
				&b,
				ObservationType::Inferred,
			)
			.expect("Could not add rate");

		let expected_output = Quant::new(2, 0);
		let result = graph.convert("EUR", "USD");
		assert!(result.is_some());

		let (out, path_len, _) = result.unwrap();

		assert_eq!(expected_output, out);
		assert_eq!(path_len, 1);
		assert!(!graph.has_inconsistent_cycle());
	}

	#[test]
	fn test_indirect_conversion() {
		let mut graph: Graph = Graph::new_undated();
		let a = Amount::new(Quant::new(2, 0), "USD");
		let b = Amount::new(Quant::new(1, 0), "EUR");
		graph
			.add_rate(
				&Date::from_str("2024-11-12").unwrap(),
				&a,
				&b,
				ObservationType::Inferred,
			)
			.expect("Could not add rate");

		let c = Amount::new(Quant::new(5, 0), "EUR");
		let d = Amount::new(Quant::new(1, 0), "GBP");
		graph
			.add_rate(
				&Date::from_str("2024-11-12").unwrap(),
				&c,
				&d,
				ObservationType::Inferred,
			)
			.expect("Could not add rate");

		let expected_output = Quant::new(1, 1);
		let result = graph.convert("USD", "GBP");
		assert!(result.is_some());

		let (out, path_len, _) = result.unwrap();

		assert_eq!(expected_output, out);
		assert_eq!(path_len, 2);
		assert!(!graph.has_inconsistent_cycle());
	}

	#[test]
	fn test_reverse_indirect_conversion() {
		let mut graph: Graph = Graph::new_undated();
		let a = Amount::new(Quant::new(2, 0), "USD");
		let b = Amount::new(Quant::new(1, 0), "EUR");
		graph
			.add_rate(
				&Date::from_str("2024-11-12").unwrap(),
				&a,
				&b,
				ObservationType::Inferred,
			)
			.expect("Could not add rate");

		let c = Amount::new(Quant::new(5, 0), "EUR");
		let d = Amount::new(Quant::new(1, 0), "GBP");
		graph
			.add_rate(
				&Date::from_str("2024-11-12").unwrap(),
				&c,
				&d,
				ObservationType::Inferred,
			)
			.expect("Could not add rate");

		let expected_output = Quant::new(10, 0);
		let result = graph.convert("GBP", "USD");
		assert!(result.is_some());

		let (out, path_len, _) = result.unwrap();

		assert_eq!(expected_output, out);
		assert_eq!(path_len, 2);
		assert!(!graph.has_inconsistent_cycle());
	}

	#[test]
	fn test_self_conversion() {
		let graph: Graph = Graph::new_undated();

		let result = graph.convert("USD", "USD");

		assert_eq!(result.unwrap().0, Quant::from_i128(1));
	}

	#[test]
	fn test_no_conversion_path() {
		let mut graph: Graph = Graph::new_undated();
		let a = Amount::new(Quant::new(2, 0), "USD");
		let b = Amount::new(Quant::new(1, 0), "EUR");
		graph
			.add_rate(
				&Date::from_str("2024-11-12").unwrap(),
				&a,
				&b,
				ObservationType::Inferred,
			)
			.expect("Could not add rate");

		let c = Amount::new(Quant::new(3, 0), "JPY");
		let d = Amount::new(Quant::new(1, 0), "INR");
		graph
			.add_rate(
				&Date::from_str("2024-11-12").unwrap(),
				&c,
				&d,
				ObservationType::Inferred,
			)
			.expect("Could not add rate");

		let result = graph.convert("USD", "JPY");

		assert!(result.is_none());
		assert!(!graph.has_inconsistent_cycle());
	}

	#[test]
	fn test_large_graph_conversion() {
		let mut graph: Graph = Graph::new_undated();
		let a = Amount::new(Quant::new(2, 0), "USD");
		let b = Amount::new(Quant::new(1, 0), "EUR");
		graph
			.add_rate(
				&Date::from_str("2024-11-12").unwrap(),
				&a,
				&b,
				ObservationType::Inferred,
			)
			.expect("Could not add rate");

		let c = Amount::new(Quant::new(5, 0), "EUR");
		let d = Amount::new(Quant::new(1, 0), "GBP");
		graph
			.add_rate(
				&Date::from_str("2024-11-12").unwrap(),
				&c,
				&d,
				ObservationType::Inferred,
			)
			.expect("Could not add rate");

		let e = Amount::new(Quant::new(10, 0), "GBP");
		let f = Amount::new(Quant::new(1, 0), "JPY");
		graph
			.add_rate(
				&Date::from_str("2024-11-12").unwrap(),
				&e,
				&f,
				ObservationType::Inferred,
			)
			.expect("Could not add rate");

		let g = Amount::new(Quant::new(3, 0), "JPY");
		let h = Amount::new(Quant::new(1, 0), "INR");
		graph
			.add_rate(
				&Date::from_str("2024-11-12").unwrap(),
				&g,
				&h,
				ObservationType::Inferred,
			)
			.expect("Could not add rate");

		let expected_output = Quant::new(300, 0);
		let result = graph.convert("INR", "USD");
		let direct_rate = graph.get_direct_rate("GBP", "EUR", false).unwrap();
		assert!(result.is_some());

		let (out, path_len, _) = result.unwrap();

		assert_eq!(expected_output, out);
		assert_eq!(path_len, 4);
		assert!(!graph.has_inconsistent_cycle());
		assert_eq!(direct_rate, Quant::new(5, 0));
	}

	#[test]
	fn test_nonexistent_currency() {
		let graph: Graph = Graph::new_undated();

		let result = graph.convert("USD", "XYZ");

		assert!(result.is_none());
	}

	#[test]
	fn test_inconsistent_cycle_detection() {
		let mut graph: Graph = Graph::new_undated();

		// Add USD <-> EUR rate
		let a = Amount::new(Quant::new(2, 0), "USD");
		let b = Amount::new(Quant::new(1, 0), "EUR");
		graph
			.add_rate(
				&Date::from_str("2024-11-12").unwrap(),
				&a,
				&b,
				ObservationType::Inferred,
			)
			.expect("Could not add rate");

		// Add EUR <-> GBP rate
		let c = Amount::new(Quant::new(5, 0), "EUR");
		let d = Amount::new(Quant::new(1, 0), "GBP");
		graph
			.add_rate(
				&Date::from_str("2024-11-12").unwrap(),
				&c,
				&d,
				ObservationType::Inferred,
			)
			.expect("Could not add rate");

		// Add GBP <-> USD rate that creates an inconsistent cycle
		let e = Amount::new(Quant::new(1, 0), "GBP");
		let f = Amount::new(Quant::new(1, 0), "USD");
		let _result = graph.add_rate(
			&Date::from_str("2024-11-12").unwrap(),
			&e,
			&f,
			ObservationType::Inferred,
		);

		// Assert that the inconsistent cycle is detected
		assert!(graph.has_inconsistent_cycle());
	}

	#[test]
	fn test_large_connected_graph_no_cycles() {
		let mut graph: Graph = Graph::new_undated();

		// NOTE: This test is long-running exponentially versus the size of
		// this value. It was fine at 200 for a while, but I got impatient
		// and reduced it to 70. The point of the test is still achieved.
		let currencies: Vec<String> =
			(1..=70).map(|i| format!("C{i:03}")).collect();

		// Add consistent bidirectional rates
		for i in 0..currencies.len() {
			for j in i + 1..currencies.len() {
				let rate1 = Quant::from_i128((i * 10 + 3) as i128);
				let rate2 = Quant::from_i128((j * 10 + 3) as i128);
				let a = Amount::new(rate1, &currencies[i]);
				let b = Amount::new(rate2, &currencies[j]);
				graph
					.add_rate(
						&Date::from_str("2024-11-12").unwrap(),
						&a,
						&b,
						ObservationType::Inferred,
					)
					.expect("Could not add rate");
			}
		}

		// Test for inconsistent cycles
		assert!(
			!graph.has_inconsistent_cycle(),
			"No inconsistent cycles expected"
		);
	}

	#[test]
	fn test_large_connected_graph_with_contextual_inconsistencies() {
		let mut graph: Graph = Graph::new_undated();

		// Add bidirectional rates between some currencies
		let pairs = vec![
			("USD", "EUR", Quant::new(120, 2)), // 1 USD = 1.2 EUR
			("EUR", "GBP", Quant::new(90, 2)),  // 1 EUR = 0.9 GBP
			("GBP", "USD", Quant::new(110, 2)), // 1 GBP = 1.1 USD (creates inconsistency)
			("JPY", "USD", Quant::new(8, 1)),   // 1 JPY = 0.8 USD
			("AUD", "JPY", Quant::new(70, 2)),  // 1 AUD = 0.7 JPY
			("CAD", "AUD", Quant::new(100, 2)), // 1 CAD = 1.0 AUD
			("CHF", "CAD", Quant::new(95, 2)),  // 1 CHF = 0.95 CAD
			("CNY", "CHF", Quant::new(10, 1)),  // 1 CNY = 0.1 CHF
			("INR", "CNY", Quant::new(6, 1)),   // 1 INR = 0.6 CNY
			("SGD", "INR", Quant::new(80, 2)),  // 1 SGD = 0.8 INR
		];

		// Add all bidirectional rates
		for (from, to, rate) in pairs {
			let amount_from = Amount::new(Quant::new(100, 0), from);
			let amount_to = Amount::new(rate, to);
			graph
				.add_rate(
					&Date::from_str("2024-11-12").unwrap(),
					&amount_from,
					&amount_to,
					ObservationType::Inferred,
				)
				.expect("Could not add rate");
		}

		// Test for inconsistent cycles
		assert!(
			graph.has_inconsistent_cycle(),
			"Expected inconsistent cycle in graph"
		);
	}

	#[test]
	fn test_disconnected_segments_with_contextual_inconsistencies() {
		let mut graph: Graph = Graph::new_undated();

		// Define three disconnected segments
		let segment1 = vec![
			("USD", "EUR", Quant::new(120, 2)),
			("EUR", "GBP", Quant::new(90, 2)),
			("GBP", "USD", Quant::new(110, 2)), // Inconsistent cycle
		];
		let segment2 = vec![
			("JPY", "AUD", Quant::new(70, 2)),
			("AUD", "CAD", Quant::new(100, 2)),
			("CAD", "JPY", Quant::new(150, 2)), // Inconsistent cycle
		];
		let segment3 = vec![
			("INR", "CNY", Quant::new(6, 1)),
			("CNY", "CHF", Quant::new(10, 1)),
			("CHF", "INR", Quant::new(50, 1)), // Inconsistent cycle
		];

		// Add rates for each segment
		for (from, to, rate) in segment1 {
			let amount_from = Amount::new(Quant::new(100, 0), from);
			let amount_to = Amount::new(rate, to);
			graph
				.add_rate(
					&Date::from_str("2024-11-12").unwrap(),
					&amount_from,
					&amount_to,
					ObservationType::Inferred,
				)
				.expect("Could not add rate");
		}
		for (from, to, rate) in segment2 {
			let amount_from = Amount::new(Quant::new(100, 0), from);
			let amount_to = Amount::new(rate, to);
			graph
				.add_rate(
					&Date::from_str("2024-11-12").unwrap(),
					&amount_from,
					&amount_to,
					ObservationType::Inferred,
				)
				.expect("Could not add rate");
		}
		for (from, to, rate) in segment3 {
			let amount_from = Amount::new(Quant::new(100, 0), from);
			let amount_to = Amount::new(rate, to);
			graph
				.add_rate(
					&Date::from_str("2024-11-12").unwrap(),
					&amount_from,
					&amount_to,
					ObservationType::Inferred,
				)
				.expect("Could not add rate");
		}

		// Test for inconsistent cycles
		assert!(
			graph.has_inconsistent_cycle(),
			"Expected inconsistent cycles in segments"
		);
	}

	#[test]
	fn test_multiple_disconnected_segments_no_cycles() {
		let mut graph: Graph = Graph::new_undated();

		// Segment 1
		let seg1 = ["USD", "EUR", "GBP"];
		for i in 0..seg1.len() {
			for j in i + 1..seg1.len() {
				let a = Amount::new(Quant::from_i128((i + 1) as i128), seg1[i]);
				let b = Amount::new(Quant::from_i128((j + 1) as i128), seg1[j]);
				graph
					.add_rate(
						&Date::from_str("2024-11-12").unwrap(),
						&a,
						&b,
						ObservationType::Inferred,
					)
					.expect("Could not add rate");
			}
		}

		// Segment 2
		let seg2 = ["JPY", "AUD", "CAD"];
		for i in 0..seg2.len() {
			for j in i + 1..seg2.len() {
				let a = Amount::new(Quant::from_i128((i + 1) as i128), seg2[i]);
				let b = Amount::new(Quant::from_i128((j + 1) as i128), seg2[j]);
				graph
					.add_rate(
						&Date::from_str("2024-11-12").unwrap(),
						&a,
						&b,
						ObservationType::Inferred,
					)
					.expect("Could not add rate");
			}
		}

		// Segment 3
		let seg3 = ["CHF", "CNY", "INR"];
		for i in 0..seg3.len() {
			for j in i + 1..seg3.len() {
				let a = Amount::new(Quant::from_i128((i + 1) as i128), seg3[i]);
				let b = Amount::new(Quant::from_i128((j + 1) as i128), seg3[j]);
				graph
					.add_rate(
						&Date::from_str("2024-11-12").unwrap(),
						&a,
						&b,
						ObservationType::Inferred,
					)
					.expect("Could not add rate");
			}
		}

		// Test for inconsistent cycles
		assert!(
			!graph.has_inconsistent_cycle(),
			"No inconsistent cycles expected"
		);
	}

	#[test]
	fn test_extreme_high_rate() {
		let mut graph: Graph = Graph::new_undated();
		let a = Amount::new(Quant::new(1, 0), "USD");
		let b = Amount::new(Quant::new(1_000_000_000_000, 0), "BTC"); // 1 USD = 1 trillion BTC
		graph
			.add_rate(
				&Date::from_str("2024-11-12").unwrap(),
				&a,
				&b,
				ObservationType::Inferred,
			)
			.expect("Could not add rate");

		let expected_output = Quant::new(1_000_000_000_000, 0);
		let result = graph.convert("USD", "BTC");
		assert!(result.is_some());

		let (out, path_len, _) = result.unwrap();

		assert_eq!(expected_output, out);
		assert_eq!(path_len, 1);
		assert!(!graph.has_inconsistent_cycle());
	}

	#[test]
	fn test_extreme_low_rate() {
		let mut graph: Graph = Graph::new_undated();
		let a = Amount::new(Quant::new(1, 0), "USD");
		let b = Amount::new(Quant::from_frac(1, 1_000_000_000_000), "BTC"); // 1 USD = 1e-12 BTC
		graph
			.add_rate(
				&Date::from_str("2024-11-12").unwrap(),
				&a,
				&b,
				ObservationType::Inferred,
			)
			.expect("Could not add rate");

		let expected_output = Quant::from_frac(1, 1_000_000_000_000);
		let result = graph.convert("USD", "BTC");
		assert!(result.is_some());

		let (out, path_len, _) = result.unwrap();

		assert_eq!(expected_output, out);
		assert_eq!(path_len, 1);
		assert!(!graph.has_inconsistent_cycle());
	}

	#[test]
	fn test_combined_extreme_rates() {
		let mut graph: Graph = Graph::new_undated();
		let a = Amount::new(Quant::new(1, 0), "USD");
		let b = Amount::new(Quant::new(1_000_000_000, 0), "BTC");
		graph
			.add_rate(
				&Date::from_str("2024-11-12").unwrap(),
				&a,
				&b,
				ObservationType::Inferred,
			)
			.expect("Could not add rate");

		let c = Amount::new(Quant::from_frac(1, 1_000_000_000), "BTC");
		let d = Amount::new(Quant::new(1, 0), "ETH");
		graph
			.add_rate(
				&Date::from_str("2024-11-12").unwrap(),
				&c,
				&d,
				ObservationType::Inferred,
			)
			.expect("Could not add rate");

		let expected_output = Quant::new(1_000_000_000_000_000_000, 0);
		let result = graph.convert("USD", "ETH");
		assert!(result.is_some());

		let (out, path_len, _) = result.unwrap();

		assert_eq!(expected_output, out);
		assert_eq!(path_len, 2);
		assert!(!graph.has_inconsistent_cycle());
	}

	#[test]
	fn test_edge_case_self_loop() {
		let mut graph: Graph = Graph::new_undated();
		let a = Amount::new(Quant::new(1, 0), "USD");
		let b = Amount::new(Quant::new(1, 0), "USD"); // Self-loop
		graph
			.add_rate(
				&Date::from_str("2024-11-12").unwrap(),
				&a,
				&b,
				ObservationType::Inferred,
			)
			.expect("Could not add rate");

		let result = graph.convert("USD", "USD");
		assert!(result.is_some());

		let (out, path_len, _) = result.unwrap();

		assert_eq!(Quant::from_i128(1), out);
		assert_eq!(path_len, 0);
		assert!(!graph.has_inconsistent_cycle());
	}

	#[test]
	fn test_bizarre_fraction_high() {
		let mut graph: Graph = Graph::new_undated();
		let a = Amount::new(Quant::from_frac(123456789, 987654321), "USD");
		let b = Amount::new(Quant::from_frac(987654321, 123456789), "EUR");
		graph
			.add_rate(
				&Date::from_str("2024-11-12").unwrap(),
				&a,
				&b,
				ObservationType::Inferred,
			)
			.expect("Could not add rate");

		let expected_output =
			Quant::from_frac(12042729108518161, 188167638891241);
		let result = graph.convert("USD", "EUR");
		assert!(result.is_some());

		let (out, path_len, _) = result.unwrap();

		assert_eq!(expected_output, out);
		assert_eq!(path_len, 1);
		assert!(!graph.has_inconsistent_cycle());
	}

	#[test]
	fn test_bizarre_fraction_low() {
		let mut graph: Graph = Graph::new_undated();
		let a = Amount::new(Quant::from_frac(1, 987654321), "USD"); // 1/987654321 USD
		let b = Amount::new(Quant::new(1, 0), "JPY"); // 1 JPY
		graph
			.add_rate(
				&Date::from_str("2024-11-12").unwrap(),
				&a,
				&b,
				ObservationType::Inferred,
			)
			.expect("Could not add rate");

		let expected_output = Quant::from_frac(1, 987654321);
		let result = graph.convert("JPY", "USD");
		assert!(result.is_some());

		let (out, path_len, _) = result.unwrap();

		assert_eq!(expected_output, out);
		assert_eq!(path_len, 1);
		assert!(!graph.has_inconsistent_cycle());
	}

	#[test]
	fn test_bizarre_fraction_chain() {
		let mut graph: Graph = Graph::new_undated();

		// Add several fractions in a chain
		let a = Amount::new(Quant::from_frac(123456789, 987654321), "USD");
		let b = Amount::new(Quant::from_frac(987654321, 123456789), "EUR");
		graph
			.add_rate(
				&Date::from_str("2024-11-12").unwrap(),
				&a,
				&b,
				ObservationType::Inferred,
			)
			.expect("Could not add rate");

		let c = Amount::new(Quant::from_frac(22222222, 33333333), "EUR");
		let d = Amount::new(Quant::from_frac(33333333, 22222222), "GBP");
		graph
			.add_rate(
				&Date::from_str("2024-11-12").unwrap(),
				&c,
				&d,
				ObservationType::Inferred,
			)
			.expect("Could not add rate");

		let expected_output =
			Quant::from_frac(108384561976663449, 752670555564964);
		let result = graph.convert("USD", "GBP");
		assert!(result.is_some());

		let (out, path_len, _) = result.unwrap();

		assert_eq!(expected_output, out);
		assert_eq!(path_len, 2);
		assert!(!graph.has_inconsistent_cycle());
	}

	#[test]
	fn test_extreme_bizarre_fraction() {
		let mut graph: Graph = Graph::new_undated();

		// Extreme fraction rates
		let a = Amount::new(Quant::from_frac(1, 1_000_000_000_007), "BTC");
		let b = Amount::new(Quant::from_frac(1_000_000_000_007, 1), "ETH");
		graph
			.add_rate(
				&Date::from_str("2024-11-12").unwrap(),
				&a,
				&b,
				ObservationType::Inferred,
			)
			.expect("Could not add rate");

		let expected_output =
			Quant::from_frac(1_000_000_000_014_000_000_000_049, 1);
		let result = graph.convert("BTC", "ETH");
		assert!(result.is_some());

		let (out, path_len, _) = result.unwrap();

		assert_eq!(expected_output, out);
		assert_eq!(path_len, 1);
		assert!(!graph.has_inconsistent_cycle());
	}

	#[test]
	fn test_bizarre_fraction_cascade() {
		let mut graph: Graph = Graph::new_undated();

		// Chain of bizarre fractions
		let a = Amount::new(Quant::from_frac(987654321, 123456789), "USD");
		let b = Amount::new(Quant::from_frac(123456789, 987654321), "EUR");
		graph
			.add_rate(
				&Date::from_str("2024-11-12").unwrap(),
				&a,
				&b,
				ObservationType::Inferred,
			)
			.expect("Could not add rate");

		let c = Amount::new(Quant::from_frac(44444444, 55555555), "EUR");
		let d = Amount::new(Quant::from_frac(55555555, 44444444), "JPY");
		graph
			.add_rate(
				&Date::from_str("2024-11-12").unwrap(),
				&c,
				&d,
				ObservationType::Inferred,
			)
			.expect("Could not add rate");

		let e = Amount::new(Quant::from_frac(22222222, 33333333), "JPY");
		let f = Amount::new(Quant::from_frac(33333333, 22222222), "AUD");
		graph
			.add_rate(
				&Date::from_str("2024-11-12").unwrap(),
				&e,
				&f,
				ObservationType::Inferred,
			)
			.expect("Could not add rate");

		let expected_output =
			Quant::from_frac(42337718750529225, 770734662945162304);
		let result = graph.convert("USD", "AUD");
		assert!(result.is_some());

		let (out, path_len, _) = result.unwrap();

		assert_eq!(expected_output, out);
		assert_eq!(path_len, 3);
		assert!(!graph.has_inconsistent_cycle());
	}

	#[cfg(test)]
	mod determinism {
		use super::*;
		use rand::Rng;
		use std::ops::Neg;

		#[test]
		fn test_determinism_of_conversion() {
			let mut rng = rand::thread_rng();
			let currencies: Vec<&str> = vec![
				"USD", "EUR", "GBP", "JPY", "AUD", "CAD", "CHF", "CNY", "INR",
				"BTC",
			];
			let num_currencies = currencies.len();

			// Generate a single set of exchange rates between currencies
			let mut rates = vec![];
			for i in 0..num_currencies {
				for j in (i + 1)..num_currencies {
					let mut rate1 = Quant::from_frac(
						rng.gen_range(1..10_000),
						rng.gen_range(1..10_000),
					);
					let mut rate2 = Quant::from_frac(
						rng.gen_range(1..10_000),
						rng.gen_range(1..10_000),
					);

					if rng.gen_bool(0.5) {
						rate1 = rate1.neg();
						rate2 = rate2.neg();
					}

					rates.push((currencies[i], currencies[j], rate1, rate2));
				}
			}

			// Select a fixed set of conversion pairs for testing
			let test_pairs = vec![
				("USD", "EUR"),
				("EUR", "GBP"),
				("GBP", "JPY"),
				("JPY", "AUD"),
				("AUD", "CAD"),
				("CAD", "CHF"),
				("CHF", "CNY"),
				("CNY", "INR"),
				("INR", "BTC"),
				("BTC", "USD"),
			];

			// Execute conversions 100 times, creating and traversing a new graph each time
			let mut all_results = vec![];

			for _ in 0..100 {
				// Create a new graph and populate it with the same rates
				let mut graph: Graph = Graph::new_undated();
				for &(base, quote, rate1, rate2) in &rates {
					let a = Amount::new(rate1, base);
					let b = Amount::new(rate2, quote);

					graph
						.add_rate(
							&Date::from_str("2024-11-12").unwrap(),
							&a,
							&b,
							ObservationType::Inferred,
						)
						.expect("Could not add rate");
				}

				// Traverse the graph for conversions
				let mut results = vec![];

				for &(base, quote) in &test_pairs {
					if let Some(rate) = graph.convert(base, quote) {
						results.push(rate);
					} else {
						panic!("Conversion failed for {base} to {quote}");
					}
				}

				all_results.push(results);
			}

			// Check if all results are identical
			for i in 1..all_results.len() {
				assert_eq!(
					all_results[0], all_results[i],
					"Determinism failed: Results differ in iteration {}",
					i
				);
			}
		}
	}
}
