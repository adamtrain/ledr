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
use serde::Deserialize;

#[derive(Debug, Default, Deserialize)]
pub struct Config {
	pub imports: Option<Imports>,
}

#[derive(Debug, Default, Deserialize)]
pub struct Imports {
	pub mercury: Option<Mercury>,
}

#[derive(Debug, Default, Deserialize)]
pub struct Mercury {
	pub api_key: Option<String>,
	pub api_key_cmd: Option<String>,
	pub api_url: Option<String>,
	pub account_prefix: Option<String>,

	/// If set, importer will only query the specified account.
	/// Can be the account nickname or account number as string.
	pub account: Option<String>,
}
