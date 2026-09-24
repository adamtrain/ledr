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

//! Fuzzy matching for finding accounts and descriptions from a few letters.

/// How well a query matched, higher being better. Every word of the query
/// must match for there to be a score at all.
pub type Score = i64;

/// A whole word or segment matched from its start, e.g. `groc` in
/// `Expenses:Food:Groceries`
pub const SEGMENT: Score = 800;
/// The query appeared somewhere, e.g. `ocer` in `Groceries`
pub const SUBSTRING: Score = 600;
/// The query's letters are the initials of segments, e.g. `efg`
pub const INITIALS: Score = 500;

/// Scores `query` against `candidate`, ignoring case. Each whitespace-
/// separated word of the query must match on its own (so `food coff` finds
/// `Expenses:Food:Coffee`), and the score is their average.
pub fn score(query: &str, candidate: &str) -> Option<Score> {
	let candidate = candidate.to_lowercase();
	let terms: Vec<String> =
		query.split_whitespace().map(str::to_lowercase).collect();
	if terms.is_empty() {
		return Some(0);
	}
	let mut total = 0;
	for term in &terms {
		total += term_score(term, &candidate)?;
	}
	Some(total / terms.len() as Score)
}

fn is_boundary(prev: Option<char>) -> bool {
	match prev {
		None => true,
		Some(c) => !c.is_alphanumeric(),
	}
}

fn term_score(term: &str, candidate: &str) -> Option<Score> {
	if candidate == term {
		return Some(1000);
	}

	// Best substring occurrence, preferring the start of a segment
	let mut best: Option<Score> = None;
	for (i, _) in candidate.match_indices(term) {
		let prev = candidate[..i].chars().next_back();
		let s = if i == 0 {
			SEGMENT + 100
		} else if is_boundary(prev) {
			// Later segments are usually the more specific part of an
			// account name, so they win ties
			SEGMENT + (i as Score).min(50)
		} else {
			SUBSTRING - (i as Score).min(50)
		};
		best = Some(best.map_or(s, |b: Score| b.max(s)));
	}
	if best.is_some() {
		return best;
	}

	// Initials, like `efc` for Expenses:Food:Coffee
	let initials: String = candidate
		.char_indices()
		.filter(|&(i, _)| is_boundary(candidate[..i].chars().next_back()))
		.map(|(_, c)| c)
		.filter(|c| c.is_alphanumeric())
		.collect();
	if initials.contains(term) && term.chars().count() > 1 {
		return Some(INITIALS);
	}

	// Any subsequence, rewarding runs and starts of words
	let chars: Vec<char> = candidate.chars().collect();
	let mut position = 0;
	let mut score: Score = 0;
	let mut previous: Option<usize> = None;
	for t in term.chars() {
		let found = (position..chars.len()).find(|&i| chars[i] == t)?;
		score += 10;
		if previous == Some(found.wrapping_sub(1)) {
			score += 15;
		}
		if found == 0 || !chars[found - 1].is_alphanumeric() {
			score += 20;
		}
		score -= (found - position) as Score;
		previous = Some(found);
		position = found + 1;
	}
	Some(score.clamp(1, SUBSTRING - 100))
}

/// Whether `query` matches `candidate` well enough to act on without
/// asking: every word appears as written or as initials, or as an
/// abbreviation within one segment that starts where the segment does
/// (`chk` for Checking, but not `gas` for OpeningBalances).
pub fn confident(query: &str, candidate: &str) -> bool {
	let candidate = candidate.to_lowercase();
	query.split_whitespace().map(str::to_lowercase).all(|term| {
		term_score(&term, &candidate).is_some_and(|s| s >= INITIALS)
			|| candidate
				.split(|c: char| !c.is_alphanumeric())
				.any(|segment| is_abbreviation(&term, segment))
	})
}

/// `chk` for `checking`: the first letters match, and the rest follow in
/// order
fn is_abbreviation(term: &str, segment: &str) -> bool {
	let mut rest = segment.chars();
	let mut term = term.chars();
	match (term.next(), rest.next()) {
		(Some(a), Some(b)) if a == b => {},
		_ => return false,
	}
	term.all(|t| rest.any(|c| c == t))
}

/// Candidates matching `query`, best first. `weight` adds a bonus per
/// candidate, e.g. for how often or recently it has been used.
pub fn rank<'a, T>(
	query: &str,
	candidates: impl IntoIterator<Item = &'a T>,
	name: impl Fn(&T) -> &str,
	weight: impl Fn(&T) -> Score,
) -> Vec<(&'a T, Score)>
where
	T: 'a,
{
	let mut ranked: Vec<(&T, Score)> = candidates
		.into_iter()
		.filter_map(|c| score(query, name(c)).map(|s| (c, s + weight(c))))
		.collect();
	ranked.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| name(a.0).cmp(name(b.0))));
	ranked
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn test_matching() {
		assert_eq!(score("coffee", "coffee"), Some(1000));
		assert!(score("groc", "Expenses:Food:Groceries").unwrap() >= SEGMENT);
		assert!(score("ocer", "Expenses:Food:Groceries").unwrap() < SEGMENT);
		assert_eq!(score("efg", "Expenses:Food:Groceries"), Some(INITIALS));
		assert!(score("chk", "Assets:Chase:Checking").is_some());
		assert!(score("xyz", "Assets:Chase:Checking").is_none());
		assert!(score("food coff", "Expenses:Food:Coffee").unwrap() >= SEGMENT);
		assert!(score("food tea", "Expenses:Food:Coffee").is_none());
		assert_eq!(score("", "anything"), Some(0));
	}

	#[test]
	fn test_confidence() {
		assert!(confident("chk", "Assets:Chase:Checking"));
		assert!(confident("visa", "Liabilities:Visa"));
		assert!(confident("efc", "Expenses:Food:Coffee"));
		assert!(!confident("gas", "Equity:OpeningBalances"));
		assert!(!confident("xyz", "Assets:Checking"));
	}

	#[test]
	fn test_ranking_prefers_better_matches() {
		let accounts = [
			"Assets:Checking",
			"Expenses:Food:Coffee",
			"Expenses:Food:Groceries",
			"Liabilities:Visa",
		];
		let ranked = rank("co", accounts.iter(), |a| a, |_| 0);
		assert_eq!(*ranked[0].0, "Expenses:Food:Coffee");
		let ranked = rank("ch", accounts.iter(), |a| a, |_| 0);
		assert_eq!(*ranked[0].0, "Assets:Checking");
	}
}
