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
use crate::util::date::Date;
use serde::{Deserialize, Serialize};

// -------------
// -- SENDING --
// -------------

#[derive(Debug, Serialize)]
pub struct AccountParams {}

#[derive(Debug, Serialize)]
pub struct AccountTransactionsParams {
	pub start: String,
	pub end: String,
}

// ---------------
// -- RECEIVING --
// ---------------

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountsHolder {
	pub accounts: Vec<Account>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Account {
	pub id: String,
	pub account_number: String,
	pub status: String, // "active" only for what we care about

	#[serde(rename = "type")]
	pub typ: String, // "mercury" only for what we care about

	pub nickname: Option<String>,
}

impl Account {
	pub fn name(&self, prefix: &str) -> String {
		if let Some(name) = &self.nickname {
			format!(
				"{}:{}",
				prefix,
				name.chars()
					.filter(|&c| !c.is_whitespace() && c != '#')
					.collect::<String>()
			)
		} else {
			prefix.to_string()
		}
	}
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct TransactionHolder {
	pub total: i64,
	pub transactions: Vec<Transaction>,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Transaction {
	#[serde(deserialize_with = "deserialize_number_as_string")]
	pub amount: String,

	pub kind: String,

	pub counterparty_name: String,
	pub counterparty_nickname: Option<String>,

	pub created_at: String,

	pub status: String,

	pub currency_exchange_info: Option<CurrencyExchangeInfo>,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct CurrencyExchangeInfo {
	pub converted_from_currency: String,
	pub converted_to_currency: String,

	#[serde(deserialize_with = "deserialize_number_as_string")]
	pub converted_from_amount: String,

	#[serde(deserialize_with = "deserialize_number_as_string")]
	pub converted_to_amount: String,
}

impl Transaction {
	pub fn name(&self, remove_whitespace: bool) -> String {
		let out = if let Some(nickname) = &self.counterparty_nickname {
			nickname.clone()
		} else {
			self.counterparty_name.clone()
		};

		if remove_whitespace {
			out.chars()
				.filter(|&c| !c.is_whitespace() && c != '#')
				.collect::<String>()
		} else {
			out.chars().filter(|&c| c != '#').collect::<String>()
		}
	}

	/// Extracts first 10 characters, which is the ISO-8601 date.
	/// Will panic if posted_at is None.
	pub fn date(&self) -> Date {
		Date::from_str(&self.created_at.clone()[..10]).unwrap()
	}
}

// Custom deserialization function
fn deserialize_number_as_string<'de, D>(
	deserializer: D,
) -> Result<String, D::Error>
where
	D: serde::Deserializer<'de>,
{
	let value = serde_json::Value::deserialize(deserializer)?;
	match value {
		serde_json::Value::Number(num) => Ok(num.to_string()),
		_ => Err(serde::de::Error::custom("expected a number")),
	}
}
