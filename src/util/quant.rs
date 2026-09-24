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
use anyhow::{Error, anyhow, bail};
use std::cmp::Ordering;
use std::fmt;
use std::hash::{Hash, Hasher};
use std::iter::Sum;
use std::ops::{
	Add, AddAssign, Div, DivAssign, Mul, MulAssign, Neg, Sub, SubAssign,
};

/// The message every arithmetic overflow panics with. The binary installs a
/// panic hook that recognises it and turns it into a readable error, so a
/// value that cannot be represented exactly is always a loud failure and
/// never a silently wrong number.
pub const OVERFLOW_MESSAGE: &str =
	"numeric overflow: a value is too large or too precise to compute exactly";

/// No report needs more decimal places than this, and asking for more only
/// risks enormous allocations when rendering.
pub const MAX_PLACES: u32 = 100;

/// A general-purpose rational number backed by a fraction of u128s. It is
/// precise for all numbers that can be reflected in that format, which vastly
/// exceeds the requirements of any human accounting. The reason it was
/// designed so rigorously is primarily due to exchange rate calculations in
/// potentially long chains of exchange rates, which is of interest to, for
/// example, traders of cryptocurrency or complex foreign exchange use cases.
///
/// Invariants, upheld by every constructor and operation:
/// - the fraction is fully reduced;
/// - zero is always `0/1` and never negative.
///
/// Because of them, structural equality is numeric equality.
#[derive(Clone, Copy, Debug)]
pub struct Quant {
	numerator: u128,
	denominator: u128,

	/// Is always false if the numerator is zero, else is intuitive.
	is_negative: bool,

	/// How many decimal places to render when asked to print. Will round with
	/// banker's rounding when underlying precision exceeds what is requested.
	///
	/// Has no effect on the underlying fraction.
	render_precision: u32,
}

#[track_caller]
fn overflow() -> ! {
	panic!("{OVERFLOW_MESSAGE}")
}

fn mul(a: u128, b: u128) -> u128 {
	a.checked_mul(b).unwrap_or_else(|| overflow())
}

fn add(a: u128, b: u128) -> u128 {
	a.checked_add(b).unwrap_or_else(|| overflow())
}

fn pow10(exp: u32) -> u128 {
	10u128.checked_pow(exp).unwrap_or_else(|| overflow())
}

/// One step of long division: for a remainder `r < d`, the digit and new
/// remainder of `10 * r / d`, computed without ever multiplying, so that it
/// works for any denominator a u128 can hold.
fn next_digit(r: u128, d: u128) -> (u8, u128) {
	let (mut digit, mut acc) = (0u8, 0u128);
	for _ in 0..10 {
		// acc + r, reduced mod d, counting each time it wraps past d
		if acc >= d - r {
			acc -= d - r;
			digit += 1;
		} else {
			acc += r;
		}
	}
	(digit, acc)
}

/// Full 256-bit product of two u128s as (high, low) halves, so that fractions
/// can be compared by cross-multiplication without any risk of overflow.
fn wide_mul(a: u128, b: u128) -> (u128, u128) {
	const MASK: u128 = u64::MAX as u128;
	let (a_hi, a_lo) = (a >> 64, a & MASK);
	let (b_hi, b_lo) = (b >> 64, b & MASK);

	let lo_lo = a_lo * b_lo;
	let hi_lo = a_hi * b_lo;
	let lo_hi = a_lo * b_hi;
	let hi_hi = a_hi * b_hi;

	let mid = (lo_lo >> 64) + (hi_lo & MASK) + (lo_hi & MASK);
	let lo = (lo_lo & MASK) | (mid << 64);
	let hi = hi_hi + (hi_lo >> 64) + (lo_hi >> 64) + (mid >> 64);
	(hi, lo)
}

impl Default for Quant {
	fn default() -> Self {
		Self::zero()
	}
}

impl Quant {
	pub fn zero() -> Self {
		Self {
			numerator: 0,
			denominator: 1,
			render_precision: 0,
			is_negative: false,
		}
	}

	/// Creates a new Quant with the given numerator and the denominator
	/// set at 10^exp where exp is the function argument of that name.
	/// Render precision is set to the exponent value, as though you were
	/// inserting a decimal point that many places from the right into
	/// the number.
	pub fn new(numerator: i128, exp: u32) -> Self {
		let mut out = Self {
			numerator: numerator.unsigned_abs(),
			denominator: pow10(exp),
			render_precision: exp,
			is_negative: numerator < 0,
		};
		out.normalize();
		out
	}

	pub fn from_frac(numerator: i128, denominator: i128) -> Self {
		if denominator == 0 {
			panic!("Denominator cannot be zero");
		}

		let mut out = Self {
			numerator: numerator.unsigned_abs(),
			denominator: denominator.unsigned_abs(),
			render_precision: 0,
			is_negative: (numerator < 0) ^ (denominator < 0),
		};
		out.normalize();
		out
	}

	pub fn from_i128(amount: i128) -> Self {
		Self {
			numerator: amount.unsigned_abs(),
			denominator: 1,
			render_precision: 0,
			is_negative: amount < 0,
		}
	}

	/// Parses a plain decimal such as `-1234.50`, `.5` or `+3`. Thousands
	/// separators are the caller's business. The render precision becomes
	/// the number of decimal places written.
	#[allow(clippy::should_implement_trait)]
	pub fn from_str(input: &str) -> Result<Self, Error> {
		let (is_negative, digits) = match input.strip_prefix('-') {
			Some(rest) => (true, rest),
			None => (false, input.strip_prefix('+').unwrap_or(input)),
		};

		let (whole, fraction) = digits.split_once('.').unwrap_or((digits, ""));
		let all_digits = |s: &str| s.bytes().all(|b| b.is_ascii_digit());
		if (whole.is_empty() && fraction.is_empty())
			|| !all_digits(whole)
			|| !all_digits(fraction)
		{
			bail!("Invalid number: {input}");
		}

		let too_big = || anyhow!("Number is too large or too precise: {input}");
		let parse = |s: &str| -> Result<u128, Error> {
			if s.is_empty() {
				Ok(0)
			} else {
				s.parse::<u128>().map_err(|_| too_big())
			}
		};

		let precision = u32::try_from(fraction.len()).map_err(|_| too_big())?;
		let scale = 10u128.checked_pow(precision).ok_or_else(too_big)?;
		let numerator = parse(whole)?
			.checked_mul(scale)
			.and_then(|n| n.checked_add(parse(fraction).ok()?))
			.ok_or_else(too_big)?;

		let mut out = Self {
			numerator,
			denominator: scale,
			render_precision: precision,
			is_negative,
		};
		out.normalize();
		Ok(out)
	}

	/// Modifies the underlying fraction to represent a value that is rounded
	/// off to the given number of decimal places when rendered as a decimal.
	/// Uses Banker's rounding (rounds to nearest, ties to even).
	pub fn round(&mut self, decimal_places: u32) {
		let places = decimal_places.min(MAX_PLACES);

		if !self.is_exact_at(places) {
			// A value that needs more digits than a u128 can hold is rounded
			// at the finest precision that fits; nothing human-readable is
			// lost, since that is still dozens of decimal places.
			let (numerator, denominator) = (0..=places)
				.rev()
				.find_map(|p| self.rounded_fraction(p))
				.unwrap_or_else(|| overflow());
			self.numerator = numerator;
			self.denominator = denominator;
			self.normalize();
		}

		self.render_precision = places;
	}

	/// True iff this value has a terminating decimal expansion that fits in
	/// the given number of places, i.e. rounding to them would change nothing.
	fn is_exact_at(&self, places: u32) -> bool {
		let mut d = self.denominator;
		let (mut twos, mut fives) = (0u32, 0u32);
		while d.is_multiple_of(2) {
			d /= 2;
			twos += 1;
		}
		while d.is_multiple_of(5) {
			d /= 5;
			fives += 1;
		}
		d == 1 && twos.max(fives) <= places
	}

	/// The magnitude of this, rounded half-to-even at `places`, as a fraction
	/// over 10^places. None if it cannot be represented at that precision.
	fn rounded_fraction(&self, places: u32) -> Option<(u128, u128)> {
		let scale = 10u128.checked_pow(places)?;
		let (whole, digits) = self.decimal_digits(places)?;
		let numerator = digits.iter().try_fold(whole, |acc, &digit| {
			acc.checked_mul(10)?.checked_add(digit as u128)
		})?;
		Some((numerator, scale))
	}

	/// Long division of the magnitude to exactly `places` decimal digits,
	/// rounded half-to-even. Returns the integer part and the digits. None
	/// only if rounding up overflows the integer part.
	fn decimal_digits(&self, places: u32) -> Option<(u128, Vec<u8>)> {
		let d = self.denominator;
		let mut whole = self.numerator / d;
		let mut remainder = self.numerator % d;

		let mut digits = Vec::with_capacity(places as usize);
		for _ in 0..places {
			let (digit, rest) = next_digit(remainder, d);
			digits.push(digit);
			remainder = rest;
		}

		// Round half to even on whatever remains below the last digit
		let rest = d - remainder;
		let last_is_odd = match digits.last() {
			Some(digit) => digit % 2 == 1,
			None => whole % 2 == 1,
		};
		if remainder > rest || (remainder == rest && last_is_odd) {
			let mut carry = true;
			for digit in digits.iter_mut().rev() {
				if *digit == 9 {
					*digit = 0;
				} else {
					*digit += 1;
					carry = false;
					break;
				}
			}
			if carry {
				whole = whole.checked_add(1)?;
			}
		}

		Some((whole, digits))
	}

	pub fn render_precision(&self) -> u32 {
		self.render_precision
	}

	pub fn set_render_precision(&mut self, precision: u32, can_decrease: bool) {
		let precision = precision.min(MAX_PLACES);
		if self.render_precision < precision || can_decrease {
			self.render_precision = precision;
		}
	}

	pub fn is_zero(&self) -> bool {
		self.numerator == 0
	}

	pub fn is_negative(&self) -> bool {
		self.is_negative
	}

	pub fn abs(&self) -> Self {
		Self {
			is_negative: false,
			..*self
		}
	}

	pub fn negate(&mut self) {
		*self = -*self;
	}

	/// A lossy floating point approximation, for proportions in visual
	/// output only. Never use this for money.
	pub fn to_f64(&self) -> f64 {
		let magnitude = self.numerator as f64 / self.denominator as f64;
		if self.is_negative {
			-magnitude
		} else {
			magnitude
		}
	}

	/// Restores the invariants: reduced fraction, and zero is `0/1` and
	/// never negative. Called after every operation that affects the
	/// fraction, which also guards against overflow in later operations.
	fn normalize(&mut self) {
		if self.numerator == 0 {
			self.denominator = 1;
			self.is_negative = false;
			return;
		}
		let gcd = Self::gcd(self.numerator, self.denominator);
		self.numerator /= gcd;
		self.denominator /= gcd;
	}

	/// Implementation of Euclid's algorithm for greatest common divisor
	fn gcd(mut a: u128, mut b: u128) -> u128 {
		while b != 0 {
			let temp = b;
			b = a % b;
			a = temp;
		}
		a
	}

	/// Takes the reciprocal. Panics on zero, like division by zero.
	pub fn recip(&self) -> Self {
		if self.numerator == 0 {
			panic!("Attempt to divide by zero");
		}
		Self {
			numerator: self.denominator,
			denominator: self.numerator,
			..*self
		}
	}

	/// Raises the precision large enough to not appear as zero when printed,
	/// unless the number is actually zero. Allows one extra digit if the
	/// number was not otherwise rendering. This should only be used for
	/// exchange rates and nothing else.
	pub fn make_visible(&mut self) {
		if self.numerator == 0 {
			return;
		}

		// Invisible at p places iff numerator * 10^p < denominator
		let invisible_at = |p: u32| {
			10u128
				.checked_pow(p)
				.and_then(|scale| self.numerator.checked_mul(scale))
				.is_some_and(|scaled| scaled < self.denominator)
		};

		let mut precision = self.render_precision;
		if invisible_at(precision) {
			while precision < MAX_PLACES && invisible_at(precision) {
				precision += 1;
			}
			// One extra digit gives a better view of a tiny number
			precision = (precision + 1).min(MAX_PLACES);
		}

		self.render_precision = precision;
	}
}

impl std::str::FromStr for Quant {
	type Err = Error;

	fn from_str(s: &str) -> Result<Self, Self::Err> {
		Quant::from_str(s)
	}
}

impl fmt::Display for Quant {
	/// Renders with thousands separators at the render precision, rounding
	/// half to even. An explicit `{:.N}` precision renders up to N places,
	/// dropping trailing zeros beyond the render precision.
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		let places = f
			.precision()
			.map_or(self.render_precision, |p| p as u32)
			.min(MAX_PLACES);

		let (whole, mut digits) = self
			.decimal_digits(places)
			.unwrap_or_else(|| (self.numerator / self.denominator, vec![]));

		if f.precision().is_some() {
			while digits.len() > self.render_precision as usize
				&& digits.last() == Some(&0)
			{
				digits.pop();
			}
		}

		let mut int_str = whole.to_string();
		let mut i = int_str.len() as isize - 3;
		while i > 0 {
			int_str.insert(i as usize, ',');
			i -= 3;
		}

		// Never print a negative sign on something that displays as zero
		let shows_nonzero = whole != 0 || digits.iter().any(|&d| d != 0);
		let sign = if self.is_negative && shows_nonzero {
			"-"
		} else {
			""
		};

		if digits.is_empty() {
			write!(f, "{sign}{int_str}")
		} else {
			let fraction: String =
				digits.iter().map(|d| char::from(b'0' + d)).collect();
			write!(f, "{sign}{int_str}.{fraction}")
		}
	}
}

// -----------------
// -- BOILERPLATE --
// -----------------

impl Add for Quant {
	type Output = Self;

	fn add(self, rhs: Self) -> Self::Output {
		let render_precision = self.render_precision.max(rhs.render_precision);

		// Special cases for zero
		if self.numerator == 0 {
			return Self {
				render_precision,
				..rhs
			};
		}
		if rhs.numerator == 0 {
			return Self {
				render_precision,
				..self
			};
		}

		// Scale numerators to the least common denominator
		let gcd = Self::gcd(self.denominator, rhs.denominator);
		let lcm = mul(self.denominator / gcd, rhs.denominator);
		let term_a = mul(self.numerator, lcm / self.denominator);
		let term_b = mul(rhs.numerator, lcm / rhs.denominator);

		let (numerator, is_negative) = if self.is_negative == rhs.is_negative {
			(add(term_a, term_b), self.is_negative)
		} else if term_a >= term_b {
			(term_a - term_b, self.is_negative)
		} else {
			(term_b - term_a, rhs.is_negative)
		};

		let mut out = Self {
			numerator,
			denominator: lcm,
			render_precision,
			is_negative,
		};
		out.normalize();
		out
	}
}

impl AddAssign for Quant {
	fn add_assign(&mut self, rhs: Self) {
		*self = *self + rhs;
	}
}

impl Sum for Quant {
	fn sum<I: Iterator<Item = Self>>(iter: I) -> Self {
		iter.fold(Quant::zero(), |acc, quant| acc + quant)
	}
}

impl Sub for Quant {
	type Output = Self;

	fn sub(self, rhs: Self) -> Self::Output {
		self + (-rhs)
	}
}

impl SubAssign for Quant {
	fn sub_assign(&mut self, rhs: Self) {
		*self = *self - rhs;
	}
}

impl Mul for Quant {
	type Output = Self;

	fn mul(self, rhs: Self) -> Self::Output {
		let render_precision = self.render_precision.max(rhs.render_precision);
		if self.numerator == 0 || rhs.numerator == 0 {
			return Self {
				render_precision,
				..Self::zero()
			};
		}

		// Cross-reduce first to limit overflow risk
		let gcd_a = Self::gcd(self.numerator, rhs.denominator);
		let gcd_b = Self::gcd(rhs.numerator, self.denominator);

		let mut out = Self {
			numerator: mul(self.numerator / gcd_a, rhs.numerator / gcd_b),
			denominator: mul(self.denominator / gcd_b, rhs.denominator / gcd_a),
			is_negative: self.is_negative ^ rhs.is_negative,
			render_precision,
		};
		out.normalize();
		out
	}
}

impl MulAssign for Quant {
	fn mul_assign(&mut self, rhs: Self) {
		*self = *self * rhs;
	}
}

impl Mul<i128> for Quant {
	type Output = Self;

	fn mul(self, rhs: i128) -> Self::Output {
		let product = self * Quant::from_i128(rhs);
		Self {
			render_precision: self.render_precision,
			..product
		}
	}
}

impl Mul<Quant> for i128 {
	type Output = Quant;

	fn mul(self, rhs: Quant) -> Self::Output {
		Quant::from_i128(self) * rhs
	}
}

impl Div for Quant {
	type Output = Self;

	// Dividing by a fraction is multiplying by its reciprocal
	#[allow(clippy::suspicious_arithmetic_impl)]
	fn div(self, rhs: Self) -> Self::Output {
		self * rhs.recip()
	}
}

impl DivAssign for Quant {
	fn div_assign(&mut self, rhs: Self) {
		*self = *self / rhs;
	}
}

impl Div<i128> for Quant {
	type Output = Self;

	fn div(self, rhs: i128) -> Self::Output {
		self / Quant::from_i128(rhs)
	}
}

impl Div<Quant> for i128 {
	type Output = Quant;

	fn div(self, rhs: Quant) -> Self::Output {
		Quant::from_i128(self) / rhs
	}
}

impl Neg for Quant {
	type Output = Self;

	fn neg(self) -> Self::Output {
		Self {
			is_negative: !self.is_negative && self.numerator != 0,
			..self
		}
	}
}

impl PartialEq for Quant {
	fn eq(&self, other: &Self) -> bool {
		// Sound because both sides are always normalized
		self.numerator == other.numerator
			&& self.denominator == other.denominator
			&& self.is_negative == other.is_negative
	}
}

impl PartialEq<i128> for Quant {
	fn eq(&self, &other: &i128) -> bool {
		self.denominator == 1
			&& self.numerator == other.unsigned_abs()
			&& self.is_negative == (other < 0)
	}
}

impl PartialEq<Quant> for i128 {
	fn eq(&self, other: &Quant) -> bool {
		other == self
	}
}

impl Eq for Quant {}

impl PartialOrd for Quant {
	fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
		Some(self.cmp(other))
	}
}

impl PartialOrd<i128> for Quant {
	fn partial_cmp(&self, other: &i128) -> Option<Ordering> {
		Some(self.cmp(&Quant::from_i128(*other)))
	}
}

impl PartialOrd<Quant> for i128 {
	fn partial_cmp(&self, other: &Quant) -> Option<Ordering> {
		Some(Quant::from_i128(*self).cmp(other))
	}
}

impl Ord for Quant {
	fn cmp(&self, other: &Self) -> Ordering {
		match (self.is_negative, other.is_negative) {
			(true, false) => return Ordering::Less,
			(false, true) => return Ordering::Greater,
			_ => {},
		};

		// Compare magnitudes exactly by cross-multiplying in 256 bits
		let magnitude = wide_mul(self.numerator, other.denominator)
			.cmp(&wide_mul(other.numerator, self.denominator));

		if self.is_negative {
			magnitude.reverse()
		} else {
			magnitude
		}
	}
}

impl Hash for Quant {
	fn hash<H: Hasher>(&self, state: &mut H) {
		self.numerator.hash(state);
		self.denominator.hash(state);
		self.is_negative.hash(state);
		// `render_precision` intentionally excluded from the hash
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	mod creation {
		use super::*;

		mod new {
			use super::*;

			#[test]
			fn test_positive_number_with_precision() {
				let quant = Quant::new(123, 2);
				assert_eq!(quant.numerator, 123);
				assert_eq!(quant.denominator, 100);
				assert_eq!(quant.render_precision, 2);
			}

			#[test]
			fn test_zero_number() {
				let quant = Quant::new(0, 5);
				assert_eq!(quant.numerator, 0);
				assert_eq!(quant.denominator, 1);
				assert_eq!(quant.render_precision, 5);
				assert!(!quant.is_negative);
			}

			#[test]
			fn test_negative_number_with_precision() {
				let quant = Quant::new(-456, 3);
				assert_eq!(quant.numerator, 57);
				assert_eq!(quant.denominator, 125);
				assert_eq!(quant.render_precision, 3);
				assert!(quant.is_negative);
			}

			#[test]
			fn test_high_precision() {
				let quant = Quant::new(789, 10);
				assert_eq!(quant.numerator, 789);
				assert_eq!(quant.denominator, 10u128.pow(10));
				assert_eq!(quant.render_precision, 10);
				assert!(!quant.is_negative);
			}

			#[test]
			fn test_no_precision() {
				let quant = Quant::new(42, 0);
				assert_eq!(quant.numerator, 42);
				assert_eq!(quant.denominator, 1);
				assert_eq!(quant.render_precision, 0);
				assert!(!quant.is_negative);
			}

			#[test]
			fn test_large_number_and_precision() {
				let quant = Quant::new(987654321, 15);
				assert_eq!(quant.numerator, 987654321);
				assert_eq!(quant.denominator, 10u128.pow(15));
				assert_eq!(quant.render_precision, 15);
				assert!(!quant.is_negative);
			}

			#[test]
			fn test_number_reduction() {
				let quant = Quant::new(200, 2);
				assert_eq!(quant.numerator, 2);
				assert_eq!(quant.denominator, 1);
				assert_eq!(quant.render_precision, 2);
				assert!(!quant.is_negative);
			}

			#[test]
			fn test_negative_number_reduction() {
				let quant = Quant::new(-200, 2);
				assert_eq!(quant.numerator, 2);
				assert_eq!(quant.denominator, 1);
				assert_eq!(quant.render_precision, 2);
				assert!(quant.is_negative);
			}

			#[test]
			fn test_zero_precision_with_large_number() {
				let quant = Quant::new(999999999999999999, 0);
				assert_eq!(quant.numerator, 999999999999999999);
				assert_eq!(quant.denominator, 1);
				assert_eq!(quant.render_precision, 0);
				assert!(!quant.is_negative);
			}

			#[test]
			fn test_large_negative_number_with_high_precision() {
				let quant = Quant::new(-123456789, 18);
				assert_eq!(quant.numerator, 123456789);
				assert_eq!(quant.denominator, 10u128.pow(18));
				assert_eq!(quant.render_precision, 18);
				assert!(quant.is_negative);
			}
		}

		mod from_frac {
			use super::*;

			#[test]
			#[should_panic(expected = "Denominator cannot be zero")]
			fn test_zero_denominator() {
				Quant::from_frac(1, 0);
			}

			#[test]
			fn test_positive_fraction() {
				let quant = Quant::from_frac(6, 8);
				assert_eq!(quant.numerator, 3);
				assert_eq!(quant.denominator, 4);
				assert_eq!(quant.render_precision, 0);
				assert!(!quant.is_negative);
			}

			#[test]
			fn test_negative_fraction_numerator() {
				let quant = Quant::from_frac(-6, 8);
				assert_eq!(quant.numerator, 3);
				assert_eq!(quant.denominator, 4);
				assert_eq!(quant.render_precision, 0);
				assert!(quant.is_negative);
			}

			#[test]
			fn test_negative_fraction_denominator() {
				let quant = Quant::from_frac(6, -8);
				assert_eq!(quant.numerator, 3);
				assert_eq!(quant.denominator, 4);
				assert_eq!(quant.render_precision, 0);
				assert!(quant.is_negative);
			}

			#[test]
			fn test_negative_fraction_both() {
				let quant = Quant::from_frac(-6, -8);
				assert_eq!(quant.numerator, 3);
				assert_eq!(quant.denominator, 4);
				assert_eq!(quant.render_precision, 0);
				assert!(!quant.is_negative);
			}

			#[test]
			fn test_reduction_to_lowest_terms() {
				let quant = Quant::from_frac(100, 400);
				assert_eq!(quant.numerator, 1);
				assert_eq!(quant.denominator, 4);
				assert_eq!(quant.render_precision, 0);
				assert!(!quant.is_negative);
			}

			#[test]
			fn test_large_numbers() {
				let quant =
					Quant::from_frac(12345678901234567890, 9876543210987654321);
				assert_eq!(quant.numerator, 137174210);
				assert_eq!(quant.denominator, 109739369);
				assert_eq!(quant.render_precision, 0);
				assert!(!quant.is_negative);
			}

			#[test]
			fn test_one_as_denominator() {
				let quant = Quant::from_frac(7, 1);
				assert_eq!(quant.numerator, 7);
				assert_eq!(quant.denominator, 1);
				assert_eq!(quant.render_precision, 0);
				assert!(!quant.is_negative);
			}

			#[test]
			fn test_zero_as_numerator() {
				let quant = Quant::from_frac(0, 5);
				assert_eq!(quant.numerator, 0);
				assert_eq!(quant.denominator, 1);
				assert_eq!(quant.render_precision, 0);
				assert!(!quant.is_negative);
			}

			#[test]
			fn test_already_reduced_fraction() {
				let quant = Quant::from_frac(3, 4);
				assert_eq!(quant.numerator, 3);
				assert_eq!(quant.denominator, 4);
				assert_eq!(quant.render_precision, 0);
				assert!(!quant.is_negative);
			}

			#[test]
			fn test_negative_one_as_denominator() {
				let quant = Quant::from_frac(7, -1);
				assert_eq!(quant.numerator, 7);
				assert_eq!(quant.denominator, 1);
				assert_eq!(quant.render_precision, 0);
				assert!(quant.is_negative);
			}

			#[test]
			fn test_negative_one_as_numerator() {
				let quant = Quant::from_frac(-1, 3);
				assert_eq!(quant.numerator, 1);
				assert_eq!(quant.denominator, 3);
				assert_eq!(quant.render_precision, 0);
				assert!(quant.is_negative);
			}

			#[test]
			fn test_minimal_fraction() {
				let quant = Quant::from_frac(1, 2);
				assert_eq!(quant.numerator, 1);
				assert_eq!(quant.denominator, 2);
				assert_eq!(quant.render_precision, 0);
				assert!(!quant.is_negative);
			}
		}

		mod from_str {
			use super::*;

			#[test]
			fn test_from_str_positive_integer() {
				let quant = Quant::from_str("123").unwrap();
				assert_eq!(quant.numerator, 123);
				assert_eq!(quant.denominator, 1);
				assert_eq!(quant.render_precision, 0);
				assert!(!quant.is_negative);
			}

			#[test]
			fn test_from_str_negative_integer() {
				let quant = Quant::from_str("-123").unwrap();
				assert_eq!(quant.numerator, 123);
				assert_eq!(quant.denominator, 1);
				assert_eq!(quant.render_precision, 0);
				assert!(quant.is_negative);
			}

			#[test]
			fn test_from_str_positive_decimal() {
				let quant = Quant::from_str("123.456").unwrap();
				assert_eq!(quant.numerator, 15432);
				assert_eq!(quant.denominator, 125);
				assert_eq!(quant.render_precision, 3);
				assert!(!quant.is_negative);
			}

			#[test]
			fn test_from_str_negative_decimal() {
				let quant = Quant::from_str("-123.456").unwrap();
				assert_eq!(quant.numerator, 15432);
				assert_eq!(quant.denominator, 125);
				assert_eq!(quant.render_precision, 3);
				assert!(quant.is_negative);
			}

			#[test]
			fn test_from_str_invalid_format() {
				let result = Quant::from_str("123.45.67");
				assert!(
					result.is_err(),
					"Expected error for invalid decimal format"
				);
			}

			#[test]
			fn test_from_str_invalid_characters() {
				let result = Quant::from_str("abc123");
				assert!(
					result.is_err(),
					"Expected error for invalid characters"
				);
			}

			#[test]
			fn test_from_str_empty_string() {
				let result = Quant::from_str("");
				assert!(result.is_err(), "Expected error for empty string");
			}

			#[test]
			fn test_from_str_zero() {
				let quant = Quant::from_str("0").unwrap();
				assert_eq!(quant.numerator, 0);
				assert_eq!(quant.denominator, 1);
				assert_eq!(quant.render_precision, 0);
				assert!(!quant.is_negative);
			}

			#[test]
			fn test_from_str_negative_zero() {
				let quant = Quant::from_str("-0").unwrap();
				assert_eq!(quant.numerator, 0);
				assert_eq!(quant.denominator, 1);
				assert_eq!(quant.render_precision, 0);
				assert!(!quant.is_negative);
			}

			#[test]
			fn test_from_str_zero_decimal() {
				let quant = Quant::from_str("0.00").unwrap();
				assert_eq!(quant.numerator, 0);
				assert_eq!(quant.denominator, 1);
				assert_eq!(quant.render_precision, 2);
				assert!(!quant.is_negative);
			}

			#[test]
			fn test_from_str_negative_zero_decimal() {
				let quant = Quant::from_str("-0.00").unwrap();
				assert_eq!(quant.numerator, 0);
				assert_eq!(quant.denominator, 1);
				assert_eq!(quant.render_precision, 2);
				assert!(!quant.is_negative);
			}

			#[test]
			fn test_from_str_near_zero_decimal() {
				let quant = Quant::from_str("0.05").unwrap();
				assert_eq!(quant.numerator, 1);
				assert_eq!(quant.denominator, 20);
				assert_eq!(quant.render_precision, 2);
				assert!(!quant.is_negative);
			}

			#[test]
			fn test_from_str_negative_near_zero_decimal() {
				let quant = Quant::from_str("-0.05").unwrap();
				assert_eq!(quant.numerator, 1);
				assert_eq!(quant.denominator, 20);
				assert_eq!(quant.render_precision, 2);
				assert!(quant.is_negative);
			}
		}

		mod from_i128 {
			use super::*;

			#[test]
			fn test_from_i128_positive() {
				let quant = Quant::from_i128(42);
				assert_eq!(quant.numerator, 42);
				assert_eq!(quant.denominator, 1);
				assert_eq!(quant.render_precision, 0);
				assert!(!quant.is_negative);
			}

			#[test]
			fn test_from_i128_negative() {
				let quant = Quant::from_i128(-42);
				assert_eq!(quant.numerator, 42);
				assert_eq!(quant.denominator, 1);
				assert_eq!(quant.render_precision, 0);
				assert!(quant.is_negative);
			}
		}
	}

	mod math {
		use super::*;

		mod add {
			use super::*;

			#[test]
			fn test_add() {
				let a = Quant::from_frac(1, 2);
				let b = Quant::from_frac(1, 3);
				assert_eq!(a + b, Quant::from_frac(5, 6));
			}

			#[test]
			fn test_add_with_integer() {
				let a = Quant::from_frac(1, 2);
				let b = Quant::from_i128(2);
				assert_eq!(a + b, Quant::from_frac(5, 2));
			}

			#[test]
			fn test_add_large_numbers() {
				let a = Quant::from_frac(123456789, 987654321);
				let b = Quant::from_frac(987654321, 123456789);
				assert_eq!(
					a + b,
					Quant::from_frac(990702636540161562, 121932631112635269)
				);
			}

			#[test]
			fn test_add_negative_numbers() {
				let a = Quant::from_frac(-1, 3);
				let b = Quant::from_frac(-2, 5);
				assert_eq!(a + b, Quant::from_frac(-11, 15));
			}

			#[test]
			fn test_add_mixed_signs() {
				let a = Quant::from_frac(5, 6);
				let b = Quant::from_frac(-1, 3);
				assert_eq!(a + b, Quant::from_frac(1, 2));
			}

			#[test]
			fn test_add_small_numbers() {
				let a = Quant::from_frac(1, 1000000);
				let b = Quant::from_frac(1, 1000000);
				assert_eq!(a + b, Quant::from_frac(1, 500000));
			}
		}

		mod add_assign {
			use super::*;

			#[test]
			fn test_add_assign() {
				let mut a = Quant::from_frac(1, 2);
				let b = Quant::from_frac(1, 3);
				a += b;
				assert_eq!(a, Quant::from_frac(5, 6));
			}

			#[test]
			fn test_add_assign_large_numbers() {
				let mut a = Quant::from_frac(123456789, 987654321);
				let b = Quant::from_frac(987654321, 123456789);
				a += b;
				assert_eq!(
					a,
					Quant::from_frac(990702636540161562, 121932631112635269)
				);
			}

			#[test]
			fn test_add_assign_negative_numbers() {
				let mut a = Quant::from_frac(-1, 3);
				let b = Quant::from_frac(-2, 5);
				a += b;
				assert_eq!(a, Quant::from_frac(-11, 15));
			}

			#[test]
			fn test_add_assign_mixed_signs() {
				let mut a = Quant::from_frac(-1, 3);
				let b = Quant::from_frac(2, 5);
				a += b;
				assert_eq!(a, Quant::from_frac(1, 15));
			}
		}

		mod sub {
			use super::*;

			#[test]
			fn test_sub() {
				let a = Quant::from_frac(3, 4);
				let b = Quant::from_frac(1, 4);
				assert_eq!(a - b, Quant::from_frac(2, 4));
			}

			#[test]
			fn test_sub_with_integer() {
				let a = Quant::from_i128(5);
				let b = Quant::from_frac(1, 2);
				assert_eq!(a - b, Quant::from_frac(9, 2));
			}

			#[test]
			fn test_sub_negative_numbers() {
				let a = Quant::from_frac(-1, 2);
				let b = Quant::from_frac(-1, 3);
				assert_eq!(a - b, Quant::from_frac(-1, 6));
			}

			#[test]
			fn test_sub_mixed_signs() {
				let a = Quant::from_frac(5, 6);
				let b = Quant::from_frac(-1, 3);
				assert_eq!(a - b, Quant::from_frac(7, 6));

				let a = Quant::from_frac(-5, 6);
				let b = Quant::from_frac(1, 3);
				assert_eq!(a - b, Quant::from_frac(-7, 6));
			}

			#[test]
			fn test_sub_small_numbers() {
				let a = Quant::from_frac(1, 1000000);
				let b = Quant::from_frac(1, 1000000);
				assert_eq!(a - b, Quant::from_frac(0, 1));
			}
		}

		mod sub_assign {
			use super::*;

			#[test]
			fn test_sub_assign() {
				let mut a = Quant::from_frac(3, 4);
				let b = Quant::from_frac(1, 4);
				a -= b;
				assert_eq!(a, Quant::from_frac(2, 4));
			}

			#[test]
			fn test_sub_assign_negative_numbers() {
				let mut a = Quant::from_frac(-1, 2);
				let b = Quant::from_frac(-1, 3);
				a -= b;
				assert_eq!(a, Quant::from_frac(-1, 6));
			}

			#[test]
			fn test_sub_assign_mixed_signs() {
				let mut a = Quant::from_frac(5, 6);
				let b = Quant::from_frac(-1, 3);
				a -= b;
				assert_eq!(a, Quant::from_frac(7, 6));
			}
		}

		mod mul {
			use super::*;

			#[test]
			fn test_mul() {
				let a = Quant::from_frac(2, 3);
				let b = Quant::from_frac(3, 4);
				assert_eq!(a * b, Quant::from_frac(6, 12));
			}

			#[test]
			fn test_mul_with_integer() {
				let a = Quant::from_frac(3, 5);
				let b = 2;
				assert_eq!(a * b, Quant::from_frac(6, 5));
			}

			#[test]
			fn test_mul_negative_numbers() {
				let a = Quant::from_frac(-2, 3);
				let b = Quant::from_frac(-3, 4);
				assert_eq!(a * b, Quant::from_frac(6, 12));
			}

			#[test]
			fn test_mul_negative_signs() {
				let a = Quant::from_frac(-2, 3);
				let b = Quant::from_frac(3, 4);
				assert_eq!(a * b, Quant::from_frac(-6, 12));

				let c = Quant::from_frac(2, 3);
				let d = Quant::from_frac(-3, 4);
				assert_eq!(c * d, Quant::from_frac(-6, 12));
			}
		}

		mod mul_assign {
			use super::*;

			#[test]
			fn test_mul_assign() {
				let mut a = Quant::from_frac(3, 4);
				let b = Quant::from_frac(2, 3);
				a *= b;
				assert_eq!(a, Quant::from_frac(6, 12));
			}

			#[test]
			fn test_mul_assign_negative_numbers() {
				let mut a = Quant::from_frac(3, 4);
				let b = Quant::from_frac(2, 3);
				a *= b;
				assert_eq!(a, Quant::from_frac(6, 12));
			}

			#[test]
			fn test_mul_assign_mixed_signs() {
				let mut a = Quant::from_frac(-3, 4);
				let b = Quant::from_frac(2, 3);
				a *= b;
				assert_eq!(a, Quant::from_frac(-6, 12));

				let mut c = Quant::from_frac(-3, 4);
				let d = Quant::from_frac(2, 3);
				c *= d;
				assert_eq!(c, Quant::from_frac(-6, 12));
			}
		}

		mod div {
			use super::*;

			#[test]
			fn test_div_large_positive_numbers() {
				let a = Quant::from_frac(98765432109876543210, 1);
				let b = Quant::from_frac(123456789, 1);
				assert_eq!(
					a / b,
					Quant::from_frac(98765432109876543210, 123456789)
				);
			}

			#[test]
			fn test_div_large_negative_numbers() {
				let a = Quant::from_frac(-98765432109876543210, 1);
				let b = Quant::from_frac(-123456789, 1);
				assert_eq!(
					a / b,
					Quant::from_frac(98765432109876543210, 123456789)
				);
			}

			#[test]
			fn test_div_small_positive_numbers() {
				let a = Quant::from_frac(1, 1000000);
				let b = Quant::from_frac(1, 1000);
				assert_eq!(a / b, Quant::from_frac(1, 1000));
			}

			#[test]
			fn test_div_small_negative_numbers() {
				let a = Quant::from_frac(-1, 1000000);
				let b = Quant::from_frac(1, 1000);
				assert_eq!(a / b, Quant::from_frac(-1, 1000));
			}

			#[test]
			fn test_div_large_and_small_numbers() {
				let a = Quant::from_frac(1000000000000000000, 1);
				let b = Quant::from_frac(1, 1000000000);
				assert_eq!(
					a / b,
					Quant::from_frac(1000000000000000000000000000, 1)
				);
			}

			#[test]
			fn test_div_precision_boundary() {
				let a =
					Quant::from_frac(123456789012345678, 1000000000000000000);
				let b = Quant::from_frac(1, 1000000000);
				assert_eq!(
					a / b,
					Quant::from_frac(123456789012345678, 1000000000)
				);
			}

			#[test]
			fn test_div_zero_dividend() {
				let a = Quant::from_frac(0, 1);
				let b = Quant::from_frac(123456789, 1);
				assert_eq!(a / b, Quant::from_frac(0, 1));
			}

			#[test]
			#[should_panic(expected = "Attempt to divide by zero")]
			fn test_div_zero_divisor() {
				let a = Quant::from_frac(123456789, 1);
				let b = Quant::from_frac(0, 1);
				let _ = a / b;
			}

			#[test]
			fn test_div_exact_result() {
				let a = Quant::from_frac(6, 1);
				let b = Quant::from_frac(2, 1);
				assert_eq!(a / b, Quant::from_frac(3, 1));
			}

			#[test]
			fn test_div_inexact_result() {
				let a = Quant::from_frac(7, 1);
				let b = Quant::from_frac(3, 1);
				assert_eq!(a / b, Quant::from_frac(7, 3));
			}

			#[test]
			fn test_div_rounding_result() {
				let a = Quant::from_frac(123456789, 1);
				let b = Quant::from_frac(1000000, 1);
				assert_eq!(a / b, Quant::from_frac(123456789, 1000000));
			}

			#[test]
			fn test_div_negative_mixed_signs() {
				let a = Quant::from_frac(-5, 2);
				let b = Quant::from_frac(3, 4);
				assert_eq!(a / b, Quant::from_frac(-20, 6));
			}

			#[test]
			fn test_div_negative_and_positive() {
				let a = Quant::from_frac(-5, 4);
				let b = Quant::from_frac(-3, 8);
				assert_eq!(a / b, Quant::from_frac(40, 12));
			}

			#[test]
			fn test_div_near_zero_positive() {
				let a = Quant::from_frac(1, 1000000000);
				let b = Quant::from_frac(1, 1000000000000000);
				assert_eq!(a / b, Quant::from_frac(1000000, 1));
			}

			#[test]
			fn test_div_near_zero_negative() {
				let a = Quant::from_frac(-1, 1000000000);
				let b = Quant::from_frac(1, 1000000000000000);
				assert_eq!(a / b, Quant::from_frac(-1000000, 1));
			}
		}

		mod div_assign {
			use super::*;

			#[test]
			fn test_div_assign_large_positive_numbers() {
				let mut a = Quant::from_frac(98765432109876543210, 1);
				let b = Quant::from_frac(123456789, 1);
				a /= b;
				assert_eq!(
					a,
					Quant::from_frac(98765432109876543210, 123456789)
				);
			}

			#[test]
			fn test_div_assign_large_negative_numbers() {
				let mut a = Quant::from_frac(-98765432109876543210, 1);
				let b = Quant::from_frac(-123456789, 1);
				a /= b;
				assert_eq!(
					a,
					Quant::from_frac(98765432109876543210, 123456789)
				);
			}

			#[test]
			fn test_div_assign_small_positive_numbers() {
				let mut a = Quant::from_frac(1, 1000000);
				let b = Quant::from_frac(1, 1000);
				a /= b;
				assert_eq!(a, Quant::from_frac(1, 1000));
			}

			#[test]
			fn test_div_assign_small_negative_numbers() {
				let mut a = Quant::from_frac(-1, 1000000);
				let b = Quant::from_frac(1, 1000);
				a /= b;
				assert_eq!(a, Quant::from_frac(-1, 1000));
			}

			#[test]
			fn test_div_assign_large_and_small_numbers() {
				let mut a = Quant::from_frac(1000000000000000000, 1);
				let b = Quant::from_frac(1, 1000000000);
				a /= b;
				assert_eq!(
					a,
					Quant::from_frac(1000000000000000000000000000, 1)
				);
			}

			#[test]
			fn test_div_assign_precision_boundary() {
				let mut a =
					Quant::from_frac(123456789012345678, 1000000000000000000);
				let b = Quant::from_frac(1, 1000000000);
				a /= b;
				assert_eq!(a, Quant::from_frac(123456789012345678, 1000000000));
			}

			#[test]
			fn test_div_assign_zero_dividend() {
				let mut a = Quant::from_frac(0, 1);
				let b = Quant::from_frac(123456789, 1);
				a /= b;
				assert_eq!(a, Quant::from_frac(0, 1));
			}

			#[test]
			#[should_panic(expected = "Attempt to divide by zero")]
			fn test_div_assign_zero_divisor() {
				let mut a = Quant::from_frac(123456789, 1);
				let b = Quant::from_frac(0, 1);
				a /= b;
			}

			#[test]
			fn test_div_assign_exact_result() {
				let mut a = Quant::from_frac(6, 1);
				let b = Quant::from_frac(2, 1);
				a /= b;
				assert_eq!(a, Quant::from_frac(3, 1));
			}

			#[test]
			fn test_div_assign_inexact_result() {
				let mut a = Quant::from_frac(7, 1);
				let b = Quant::from_frac(3, 1);
				a /= b;
				assert_eq!(a, Quant::from_frac(7, 3));
			}

			#[test]
			fn test_div_assign_rounding_result() {
				let mut a = Quant::from_frac(123456789, 1);
				let b = Quant::from_frac(1000000, 1);
				a /= b;
				assert_eq!(a, Quant::from_frac(123456789, 1000000));
			}

			#[test]
			fn test_div_assign_negative_mixed_signs() {
				let mut a = Quant::from_frac(-5, 2);
				let b = Quant::from_frac(3, 4);
				a /= b;
				assert_eq!(a, Quant::from_frac(-20, 6));
			}

			#[test]
			fn test_div_assign_negative_and_positive() {
				let mut a = Quant::from_frac(-5, 4);
				let b = Quant::from_frac(-3, 8);
				a /= b;
				assert_eq!(a, Quant::from_frac(40, 12));
			}

			#[test]
			fn test_div_assign_near_zero_positive() {
				let mut a = Quant::from_frac(1, 1000000000);
				let b = Quant::from_frac(1, 1000000000000000);
				a /= b;
				assert_eq!(a, Quant::from_frac(1000000, 1));
			}

			#[test]
			fn test_div_assign_near_zero_negative() {
				let mut a = Quant::from_frac(-1, 1000000000);
				let b = Quant::from_frac(1, 1000000000000000);
				a /= b;
				assert_eq!(a, Quant::from_frac(-1000000, 1));
			}
		}

		mod negation {
			use super::*;

			#[test]
			fn test_negation() {
				let a = Quant::from_frac(3, 4);
				assert_eq!(-a, Quant::from_frac(-3, 4));

				let a = Quant::from_frac(-3, 4);
				assert_eq!(-a, Quant::from_frac(3, 4));

				let a = Quant::from_frac(-3, -4);
				assert_eq!(-a, Quant::from_frac(-3, 4));
			}
		}

		mod operation_order {
			use super::*;

			#[test]
			fn test_associative_property_with_multiplication_and_division() {
				let a = Quant::from_frac(123456789, 987654321);
				let b = Quant::from_frac(987654321, 123456789);
				let c = Quant::from_frac(246813578, 864197532);
				let d = Quant::from_frac(1000000, 1000001);

				let result_1 = (a * b) / (c * d);
				let result_2 = (a / c) * (b / d);

				assert_eq!(
					result_1, result_2,
					"Order of operations affected the result"
				);
			}

			#[test]
			fn test_chained_multiplications_consistency() {
				let a = Quant::from_frac(99999999, 11111111);
				let b = Quant::from_frac(123456789, 987654321);
				let c = Quant::from_frac(1, 2);
				let d = Quant::from_frac(3, 4);
				let e = Quant::from_frac(5, 6);

				let mut result_1 = e * d * c * b * a;
				let mut result_2 = a * b * c * d * e;

				result_1.normalize();
				result_2.normalize();

				assert_eq!(
					result_1, result_2,
					"Order of chained multiplications affected the result"
				);
			}

			#[test]
			fn test_large_numbers_multiplication_division_order() {
				let a = Quant::from_frac(10i128.pow(18), 1);
				let b = Quant::from_frac(10i128.pow(9), 1);
				let c = Quant::from_frac(1, 10i128.pow(9));
				let d = Quant::from_frac(1, 10i128.pow(18));

				let result_1 = ((a / b) * c) / d;
				let result_2 = a * (c / (b * d));

				assert_eq!(
					result_1, result_2,
					"Order of operations with large numbers affected the result"
				);
			}

			#[test]
			fn test_small_numbers_multiplication_division_order() {
				let a = Quant::from_frac(1, 10i128.pow(12));
				let b = Quant::from_frac(10i128.pow(6), 1);
				let c = Quant::from_frac(1, 10i128.pow(6));
				let d = Quant::from_frac(10i128.pow(3), 10i128.pow(9));

				let result_1 = (a * b / c) * d;
				let result_2 = ((a * d) / c) * b;

				assert_eq!(
					result_1, result_2,
					"Order of operations with small numbers affected the result"
				);
			}

			#[test]
			fn test_mixed_large_and_small_numbers_order() {
				let a = Quant::from_frac(10i128.pow(18), 1);
				let b = Quant::from_frac(1, 10i128.pow(12));
				let c = Quant::from_frac(123456, 987654321);
				let d = Quant::from_frac(987654321, 123456);
				let e = Quant::from_frac(10i128.pow(6), 10i128.pow(9));

				let result_1 = (a * b / c) * (d / e);
				let result_2 = ((a / c) * d / e) * b;

				assert_eq!(
					result_1, result_2,
					"Order of operations with mixed large and small numbers affected the result"
				);
			}
		}
	}

	mod ordering {
		use super::*;

		#[test]
		fn test_quant_greater_equal() {
			let a = Quant::from_frac(5, 2);
			let b = Quant::from_frac(10, 4);
			let c = Quant::from_frac(6, 2);
			let d = Quant::from_frac(4, 2);

			assert!(a >= b, "Expected a >= b (both equal to 2.5)");
			assert!(c >= a, "Expected c >= a (3.0 >= 2.5)");
			assert!(d < a, "Expected d < a (2.0 < 2.5)");
		}

		#[test]
		fn test_quant_less_equal() {
			let a = Quant::from_frac(5, 2);
			let b = Quant::from_frac(10, 4);
			let c = Quant::from_frac(6, 2);
			let d = Quant::from_frac(4, 2);

			assert!(a <= b, "Expected a <= b (both equal to 2.5)");
			assert!(a <= c, "Expected a <= c (2.5 <= 3.0)");
			assert!(a > d, "Expected a > d (2.5 > 2.0)");
		}

		#[test]
		fn test_quant_equal_i128() {
			let quant = Quant::from_frac(10, 2);
			let int_value: i128 = 5;

			assert!(quant == int_value, "Expected quant == int_value");
		}

		#[test]
		fn test_i128_equal_quant() {
			let quant = Quant::from_frac(10, 2);
			let int_value: i128 = 5;

			assert_eq!(int_value, quant, "Expected int_value == quant");
		}

		#[test]
		fn test_quant_partial_ord_i128() {
			let quant = Quant::from_frac(15, 2);
			let int_value: i128 = 8;

			assert!(quant < int_value, "Expected quant < int_value");
			assert!(int_value > quant, "Expected int_value > quant");
		}

		#[test]
		fn test_quant_partial_ord() {
			let a = Quant::from_frac(7, 2);
			let b = Quant::from_frac(9, 2);
			let c = Quant::from_frac(14, 4);

			assert!(a < b, "Expected a < b (3.5 < 4.5)");
			assert!(b > a, "Expected b > a (4.5 > 3.5)");
			assert_eq!(a, c, "Expected a == c (3.5 == 3.5)");
		}

		#[test]
		fn test_quant_negative_ordering() {
			let a = Quant::from_frac(-5, 2);
			let b = Quant::from_frac(-10, 4);
			let c = Quant::from_frac(-6, 2);
			let d = Quant::from_frac(-4, 2);

			assert!(a >= b, "Expected a >= b (both equal to -2.5)");
			assert!(a > c, "Expected a > c (-2.5 > -3.0)");
			assert!(a <= d, "Expected a <= d (-2.5 <= -2.0)");
		}

		#[test]
		fn test_quant_abs_ordering() {
			let a = Quant::from_frac(-5, 2);
			let b = Quant::from_frac(5, 2);
			let c = Quant::from_frac(-6, 2);
			let d = Quant::from_frac(6, 2);

			assert_eq!(a.abs(), b.abs(), "Expected |a| == |b|");
			assert_eq!(c.abs(), d.abs(), "Expected |c| == |d|");
			assert!(
				c.abs() < d.abs() + Quant::from_i128(1),
				"Expected |c| < |d| + 1"
			);
		}
	}

	mod rounding {
		use super::*;

		#[test]
		fn test_round_basic() {
			let mut quant = Quant {
				numerator: 15,
				denominator: 10,
				is_negative: false,
				render_precision: 0,
			};
			quant.round(0);
			assert_eq!(quant.numerator, 2);
			assert_eq!(quant.denominator, 1);
		}

		#[test]
		fn test_round_half_to_even() {
			let mut quant = Quant {
				numerator: 155,
				denominator: 100,
				is_negative: false,
				render_precision: 0,
			};
			quant.round(1);
			assert_eq!(quant.numerator, 8);
			assert_eq!(quant.denominator, 5);
		}

		#[test]
		fn test_round_half_to_odd() {
			let mut quant = Quant {
				numerator: 254,
				denominator: 100,
				is_negative: false,
				render_precision: 0,
			};
			quant.round(1);
			assert_eq!(quant.numerator, 5);
			assert_eq!(quant.denominator, 2);
		}

		#[test]
		fn test_round_precision_0() {
			let mut quant = Quant {
				numerator: 7,
				denominator: 3,
				is_negative: false,
				render_precision: 0,
			};
			quant.round(0);
			assert_eq!(quant.numerator, 2);
			assert_eq!(quant.denominator, 1);
		}

		#[test]
		fn test_round_negative() {
			let mut quant = Quant {
				numerator: 7,
				denominator: 3,
				is_negative: true,
				render_precision: 0,
			};
			quant.round(0);
			assert_eq!(quant.numerator, 2);
			assert!(quant.is_negative);
		}

		#[test]
		fn test_reduce_after_round() {
			let mut quant = Quant {
				numerator: 200,
				denominator: 100,
				is_negative: false,
				render_precision: 0,
			};
			quant.round(0);
			assert_eq!(quant.numerator, 2);
			assert_eq!(quant.denominator, 1);
		}

		#[test]
		fn test_round_to_integer_no_string() {
			let mut quant = Quant::from_frac(123456, 1000);
			quant.round(0);
			assert_eq!(
				quant.numerator, 123,
				"Numerator should be 123 after rounding to 0 decimals"
			);
			assert_eq!(
				quant.denominator, 1,
				"Denominator should be 1 after rounding to 0 decimals"
			);
			assert!(!quant.is_negative, "quant should not be negative");
		}

		#[test]
		fn test_round_to_two_decimals_no_string() {
			let mut quant = Quant::from_frac(123456, 1000);
			quant.round(2);
			assert_eq!(
				quant.numerator, 6173,
				"Numerator should be 6173 after rounding to 2 decimals"
			);
			assert_eq!(
				quant.denominator, 50,
				"Denominator should be 50 after rounding to 2 decimals"
			);
			assert!(!quant.is_negative, "quant should not be negative");
		}

		#[test]
		fn test_bankers_rounding_down_no_string() {
			let mut quant = Quant::from_frac(123445, 1000);
			quant.round(2);
			assert_eq!(
				quant.numerator, 3086,
				"Numerator should be 3086 due to Banker's rounding"
			);
			assert_eq!(
				quant.denominator, 25,
				"Denominator should remain scaled correctly"
			);
			assert!(!quant.is_negative, "quant should not be negative");
		}

		#[test]
		fn test_bankers_rounding_up_no_string() {
			let mut quant = Quant::from_frac(123455, 1000);
			quant.round(2);
			assert_eq!(
				quant.numerator, 6173,
				"Numerator should be 6173 due to Banker's rounding"
			);
			assert_eq!(
				quant.denominator, 50,
				"Denominator should remain scaled correctly"
			);
			assert!(!quant.is_negative, "quant should not be negative");
		}

		#[test]
		fn test_round_negative_to_integer_no_string() {
			let mut quant = Quant::from_frac(-123456, 1000);
			quant.round(0);
			assert_eq!(
				quant.numerator, 123,
				"Numerator should be 123 after rounding"
			);
			assert_eq!(
				quant.denominator, 1,
				"Denominator should be 1 after rounding"
			);
			assert!(quant.is_negative, "quant should be negative");
		}

		#[test]
		fn test_round_negative_to_one_decimal_no_string() {
			let mut quant = Quant::from_frac(-123456, 1000);
			quant.round(1);
			assert_eq!(
				quant.numerator, 247,
				"Numerator should be 247 after rounding"
			);
			assert_eq!(
				quant.denominator, 2,
				"Denominator should be 2 after rounding to 1 decimal"
			);
			assert!(quant.is_negative, "quant should be negative");
		}

		#[test]
		fn test_round_large_number_no_string() {
			let mut quant = Quant::from_frac(123456789987654321, 1000000000);
			quant.round(6);
			assert_eq!(
				quant.numerator, 61728394993827,
				"Numerator should match rounded value"
			);
			assert_eq!(
				quant.denominator, 500000,
				"Denominator should match scaled precision"
			);
			assert!(!quant.is_negative, "quant should not be negative");
		}

		#[test]
		fn test_round_small_number_up_no_string() {
			let mut quant = Quant::from_frac(-5, 10000);
			quant.round(3);
			assert_eq!(
				quant.numerator, 0,
				"Numerator should be 0 after rounding down"
			);
			assert_eq!(
				quant.denominator, 1,
				"Denominator should be set to one when numerator is zero"
			);
			assert!(!quant.is_negative, "quant should not be negative");
		}

		#[test]
		fn test_round_small_number_down_no_string() {
			let mut quant = Quant::from_frac(49, 100000);
			quant.round(3);
			assert_eq!(
				quant.numerator, 0,
				"Numerator should be 0 after rounding down"
			);
			assert_eq!(
				quant.denominator, 1,
				"Denominator should simplify to 1 for zero value"
			);
			assert!(!quant.is_negative, "quant should not be negative");
		}

		#[test]
		fn test_round_to_integer() {
			let mut quant = Quant::from_str("123.456").unwrap();
			quant.round(0);
			assert_eq!(
				quant.to_string(),
				"123",
				"Expected 123 after rounding to 0 decimals"
			);
		}

		#[test]
		fn test_round_to_two_decimals() {
			let mut quant = Quant::from_str("123.456").unwrap();
			quant.round(2);
			assert_eq!(
				quant.to_string(),
				"123.46",
				"Expected 123.46 after rounding to 2 decimals"
			);
		}

		#[test]
		fn test_bankers_rounding_down() {
			let mut quant = Quant::from_str("123.445").unwrap();
			quant.round(2);
			assert_eq!(
				quant.to_string(),
				"123.44",
				"Expected 123.44 due to Banker's rounding (tie to even)"
			);
		}

		#[test]
		fn test_bankers_rounding_up() {
			let mut quant = Quant::from_str("123.455").unwrap();
			quant.round(2);
			assert_eq!(
				quant.to_string(),
				"123.46",
				"Expected 123.46 due to Banker's rounding (tie to even)"
			);
		}

		#[test]
		fn test_round_negative_to_integer() {
			let mut quant = Quant::from_str("-123.456").unwrap();
			quant.round(0);
			assert_eq!(
				quant.to_string(),
				"-123",
				"Expected -123 after rounding to 0 decimals"
			);
		}

		#[test]
		fn test_round_negative_to_one_decimal() {
			let mut quant = Quant::from_str("-123.456").unwrap();
			quant.round(1);
			assert_eq!(
				quant.to_string(),
				"-123.5",
				"Expected -123.5 after rounding to 1 decimal"
			);
		}

		#[test]
		fn test_round_large_number() {
			let mut quant = Quant::from_str("123456789.987654321").unwrap();
			quant.round(6);
			assert_eq!(
				quant.to_string(),
				"123,456,789.987654",
				"Expected 123,456,789.987654 after rounding to 6 decimals"
			);
		}

		#[test]
		fn test_round_zero() {
			let mut quant = Quant::from_str("0.0005").unwrap();
			quant.round(3);
			assert_eq!(
				quant.to_string(),
				"0.000",
				"Expected bankers rounding to bring us back to zero"
			);
		}

		#[test]
		fn test_round_small_number_down() {
			let mut quant = Quant::from_str("0.00049").unwrap();
			quant.round(3);
			assert_eq!(
				quant.to_string(),
				"0.000",
				"Expected 0.000 after rounding down small value"
			);
		}

		#[test]
		fn test_rounding_error_for_one_third() {
			let mut fraction = Quant {
				numerator: 1,
				denominator: 3,
				is_negative: false,
				render_precision: 0,
			};

			let original = fraction;
			fraction.round(2);
			let rounding_error = fraction - original;

			let expected_rounded = Quant {
				numerator: 33,
				denominator: 100,
				is_negative: false,
				render_precision: 2,
			};

			let expected_error = Quant {
				numerator: 1,
				denominator: 300,
				is_negative: true,
				render_precision: 0,
			};

			assert_eq!(
				fraction, expected_rounded,
				"The fraction was not rounded correctly."
			);

			assert_eq!(
				rounding_error, expected_error,
				"The rounding error is incorrect."
			);
		}

		#[test]
		fn test_bankers_rounding_high_prec() {
			let mut a = Quant::from_str("1074.96875").unwrap();
			a.round(2);
			assert_eq!(a.to_string(), "1,074.97")
		}
	}

	mod extremes {
		use super::*;

		#[test]
		fn test_large_numbers() {
			let quant = Quant::from_frac(i128::MAX, 1);
			assert_eq!(
				quant.numerator, 170141183460469231731687303715884105727,
				"Reduction should not occur with 1 denominator"
			);
			assert_eq!(quant.denominator, 1, "Denominator should remain 1");
			assert!(!quant.is_negative, "Quant should not be negative");
		}

		#[test]
		fn test_large_fraction() {
			let quant = Quant::from_frac(i128::MAX, i128::MAX / 10);
			assert_eq!(
				quant.numerator, 170141183460469231731687303715884105727,
				"Numerator should have been reduced once"
			);
			assert_eq!(
				quant.denominator, 17014118346046923173168730371588410572,
				"Denominator should be one order of magnitude lesser"
			);
			assert!(!quant.is_negative, "Quant should not be negative");
		}

		#[test]
		fn test_bizarre_fractions() {
			let quant = Quant::from_frac(17190837190231, 1837619237101091);
			assert_eq!(quant.numerator, 904780904749);
			assert_eq!(quant.denominator, 96716801952689);
			assert!(!quant.is_negative, "Quant should not be negative");
		}

		#[test]
		fn test_large_number_with_high_precision() {
			let quant = Quant::new(i128::MAX, 20);
			assert_eq!(
				quant.numerator,
				i128::MAX as u128,
				"Numerator should match the maximum i128 value"
			);
			assert_eq!(
				quant.denominator,
				10u128.pow(20),
				"Denominator should match the specified precision"
			);
			assert_eq!(
				quant.render_precision, 20,
				"Render precision should be 20"
			);
			assert!(!quant.is_negative, "Quant should not be negative");
		}

		#[test]
		fn test_large_number_reduction() {
			let quant = Quant::from_frac(i128::MAX, i128::MAX);
			assert_eq!(quant.numerator, 1, "Numerator should reduce to 1");
			assert_eq!(quant.denominator, 1, "Denominator should reduce to 1");
		}

		#[test]
		fn test_large_negative_number() {
			let quant = Quant::from_frac(-i128::MAX, 10);
			assert_eq!(
				quant.numerator,
				i128::MAX as u128,
				"Numerator should be the absolute value of i128::MAX"
			);
			assert_eq!(
				quant.denominator, 10,
				"Denominator should remain as specified"
			);
			assert!(quant.is_negative, "Quant should be negative");
		}

		#[test]
		fn test_small_fraction_high_precision() {
			let quant = Quant::from_frac(1, 10i128.pow(30));
			assert_eq!(
				quant.numerator, 1,
				"Numerator should remain 1 for smallest fraction"
			);
			assert_eq!(
				quant.denominator,
				10u128.pow(30),
				"Denominator should match the specified precision"
			);
			assert!(!quant.is_negative, "Quant should not be negative");
		}

		#[test]
		fn test_small_fraction_operations() {
			let a = Quant::from_frac(100, 10i128.pow(11));
			let b = Quant::from_frac(1100, 10i128.pow(12) + 13);
			let result = a * b;
			assert_eq!(result.numerator, 11);
			assert_eq!(result.denominator, 10000000000130000000);
		}

		#[test]
		fn test_large_and_small_mixed_operations() {
			let a = Quant::from_frac(i128::MAX, 1);
			let b = Quant::from_frac(2, 3);
			let result = a * b;
			assert_eq!(
				result.numerator, 340282366920938463463374607431768211454,
				"Reduction should occur prior to multiplication and not overflow"
			);
			assert_eq!(
				result.denominator, 3,
				"Resulting denominator should scale with the smaller fraction"
			);
		}

		#[test]
		fn test_reduce_very_large_fraction() {
			let mut quant = Quant::from_frac(i128::MAX - 113, i128::MAX - 1);
			quant.normalize();
			assert_eq!(quant.numerator, 12152941675747802266549093122563150401);
			assert_eq!(
				quant.denominator,
				12152941675747802266549093122563150409
			);
		}

		#[test]
		fn test_compare_values_too_large_to_cross_multiply() {
			let a = Quant::from_frac(i128::MAX, 3);
			let b = Quant::from_frac(i128::MAX - 2, 3);
			assert!(a > b);
			assert!(-a < -b);
			assert_eq!(a.cmp(&a), Ordering::Equal);
		}
	}

	/// Differential tests against arbitrary-precision rationals, so every
	/// operation is checked against an independent reference implementation.
	mod reference {
		use super::*;
		use num_bigint::BigInt;
		use num_rational::BigRational;
		use rand::Rng;

		fn big(q: &Quant) -> BigRational {
			let r = BigRational::new(
				BigInt::from(q.numerator),
				BigInt::from(q.denominator),
			);
			if q.is_negative { -r } else { r }
		}

		fn random_quant(rng: &mut impl Rng) -> Quant {
			let numerator: i128 =
				rng.random_range(-10i128.pow(15)..10i128.pow(15));
			if rng.random_bool(0.5) {
				// A typical decimal amount, as found in a ledger
				Quant::new(numerator, rng.random_range(0..9))
			} else {
				// An arbitrary fraction, as produced by exchange rates
				Quant::from_frac(numerator, rng.random_range(1..1_000_000))
			}
		}

		/// Round half to even, the slow and obvious way
		fn round_reference(x: &BigRational, places: u32) -> BigRational {
			let scale = BigRational::from_integer(BigInt::from(10).pow(places));
			let scaled = x * &scale;
			let floor = scaled.floor();
			let fraction = &scaled - &floor;
			let half = BigRational::new(BigInt::from(1), BigInt::from(2));
			let one = BigRational::from_integer(BigInt::from(1));
			let two = BigInt::from(2);
			let rounded = if fraction > half
				|| (fraction == half
					&& floor.to_integer() % &two != BigInt::from(0))
			{
				floor + one
			} else {
				floor
			};
			rounded / scale
		}

		#[test]
		fn test_arithmetic_matches_reference() {
			let mut rng = rand::rng();
			for _ in 0..4_000 {
				let a = random_quant(&mut rng);
				let b = random_quant(&mut rng);
				assert_eq!(big(&(a + b)), big(&a) + big(&b), "{a:?} + {b:?}");
				assert_eq!(big(&(a - b)), big(&a) - big(&b), "{a:?} - {b:?}");
				assert_eq!(big(&(a * b)), big(&a) * big(&b), "{a:?} * {b:?}");
				if !b.is_zero() {
					assert_eq!(
						big(&(a / b)),
						big(&a) / big(&b),
						"{a:?} / {b:?}"
					);
				}
				assert_eq!(a.cmp(&b), big(&a).cmp(&big(&b)), "{a:?} <=> {b:?}");
				assert_eq!(a == b, big(&a) == big(&b), "{a:?} == {b:?}");
			}
		}

		#[test]
		fn test_rounding_matches_reference() {
			let mut rng = rand::rng();
			for _ in 0..4_000 {
				let original = random_quant(&mut rng);
				let places = rng.random_range(0..7);
				let mut rounded = original;
				rounded.round(places);
				let error = rounded - original;
				assert_eq!(
					big(&rounded),
					round_reference(&big(&original), places),
					"{original:?} rounded to {places}"
				);
				assert_eq!(big(&error), big(&rounded) - big(&original));
			}
		}

		#[test]
		fn test_display_matches_rounding() {
			let mut rng = rand::rng();
			for _ in 0..2_000 {
				let original = random_quant(&mut rng);
				let places = rng.random_range(0..7);
				let mut shown = original;
				shown.set_render_precision(places, true);
				let mut rounded = original;
				rounded.round(places);
				assert_eq!(shown.to_string(), rounded.to_string());
			}
		}
	}

	/// Each of these reproduces a bug that once shipped.
	mod regressions {
		use super::*;

		fn rounded(q: Quant, places: u32) -> String {
			let mut q = q;
			q.round(places);
			q.to_string()
		}

		#[test]
		fn test_odd_denominator_midpoint_rounds_up() {
			// 2.6 used to round to 2, and 200/3 to 66.66
			assert_eq!(rounded(Quant::from_str("2.6").unwrap(), 0), "3");
			assert_eq!(rounded(Quant::from_str("0.6").unwrap(), 0), "1");
			assert_eq!(rounded(Quant::from_frac(8, 3), 0), "3");
			assert_eq!(rounded(Quant::from_frac(200, 3), 2), "66.67");
			assert_eq!(rounded(Quant::from_frac(-200, 3), 2), "-66.67");
		}

		#[test]
		fn test_display_rounds_rather_than_truncates() {
			let mut two_thirds = Quant::from_frac(2, 3);
			two_thirds.set_render_precision(2, true);
			assert_eq!(two_thirds.to_string(), "0.67");
		}

		#[test]
		fn test_no_negative_zero() {
			assert_eq!(-Quant::zero(), Quant::zero());
			assert_eq!((Quant::zero() - Quant::zero()).to_string(), "0");
			let mut tiny = Quant::from_str("-0.001").unwrap();
			tiny.set_render_precision(2, true);
			assert_eq!(tiny.to_string(), "0.00");
		}

		#[test]
		fn test_integer_divided_by_quant() {
			assert_eq!(10 / Quant::from_i128(4), Quant::from_frac(5, 2));
		}

		#[test]
		fn test_unrepresentable_input_is_an_error_not_a_panic() {
			let tiny = format!("0.{}1", "0".repeat(40));
			assert!(Quant::from_str(&tiny).is_err());
			assert!(Quant::from_str(&"9".repeat(50)).is_err());
		}

		#[test]
		fn test_number_syntax() {
			assert_eq!(Quant::from_str(".5").unwrap(), Quant::from_frac(1, 2));
			assert_eq!(Quant::from_str("5.").unwrap(), Quant::from_i128(5));
			assert_eq!(Quant::from_str("+5").unwrap(), Quant::from_i128(5));
			for bad in ["", "-", ".", "--5", "1e5", "1.2.3", "5-", "1 000"] {
				assert!(Quant::from_str(bad).is_err(), "{bad} should fail");
			}
		}

		#[test]
		fn test_huge_denominators_still_print_and_round_correctly() {
			// A denominator above u128::MAX / 10, as long chains of exchange
			// rates can produce; this once printed 1.653 as 2. 2^127 - 1 is
			// prime, so these fractions cannot be reduced.
			let d: i128 = i128::MAX;
			let q = Quant::from_frac(d / 1000 * 660, d);
			assert!(q.denominator > u128::MAX / 10, "exercises the wide path");
			let mut shown = q;
			shown.set_render_precision(3, true);
			assert_eq!(shown.to_string(), "0.660");
			let mut rounded = q;
			rounded.round(2);
			assert_eq!(rounded, Quant::from_frac(66, 100));
			let mut negative = -Quant::from_frac(d - 5, d);
			negative.set_render_precision(3, true);
			assert_eq!(negative.to_string(), "-1.000");
		}

		#[test]
		fn test_next_digit_matches_arithmetic() {
			for (r, d) in [(0u128, 7u128), (3, 7), (6, 7), (1, 2), (99, 100)] {
				assert_eq!(
					next_digit(r, d),
					(((r * 10) / d) as u8, (r * 10) % d)
				);
			}
			let d = u128::MAX - 1;
			let (digit, rest) = next_digit(d - 1, d);
			assert_eq!(digit, 9);
			assert!(rest < d);
		}

		#[test]
		fn test_huge_precision_is_clamped() {
			let mut q = Quant::from_frac(1, 3);
			q.round(u32::MAX);
			assert_eq!(q.render_precision(), MAX_PLACES);
		}
	}

	mod other {
		use super::*;

		#[test]
		fn test_display() {
			let money = Quant::from_str("12345.6789").unwrap();
			assert_eq!(money.to_string(), "12,345.6789");

			let negative_money = Quant::from_str("-1000000.50").unwrap();
			assert_eq!(negative_money.to_string(), "-1,000,000.50");

			let zero_money = Quant::from_str("0.00").unwrap();
			assert_eq!(zero_money.to_string(), "0.00")
		}
	}
}
