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
use anyhow::bail;
use reqwest::Method;
use serde::{Deserialize, Serialize};

pub struct Client {
	client: reqwest::blocking::Client,
	base_url: String,
	api_key: String,
}

impl Client {
	pub fn new(base_url: &str, api_key: String) -> Self {
		Client {
			client: reqwest::blocking::Client::new(),
			base_url: base_url.to_string(),
			api_key,
		}
	}

	/// Sends a GET and handle the response. Errors on non-2xx response codes.
	pub fn get<Q, R>(
		&self,
		endpoint: &str,
		query_params: Option<Q>,
	) -> Result<R, anyhow::Error>
	where
		Q: Serialize,
		R: for<'de> Deserialize<'de>,
	{
		let url = format!("{}/{}", self.base_url, endpoint);

		let mut request = self
			.client
			.request(Method::GET, &url)
			.header("Authorization", format!("Bearer {}", self.api_key));

		if let Some(query_params) = query_params {
			request = request.query(&query_params);
		}

		println!("Sending GET to {}", url);
		let response = request.send()?;

		// Handle non-2xx response codes
		if !response.status().is_success() {
			bail!("Request failed with status: {}", response.status());
		}

		let response_data: R = response.json()?;
		Ok(response_data)
	}
}
