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
use std::fs;
use std::process::Command;

/// Dynamically collects test cases from a given directory.
fn collect_test_cases(subfolder: &str) -> Vec<(String, String)> {
	let dir_path = format!("tests/test_data/{}", subfolder);

	let mut test_cases = vec![];

	if let Ok(entries) = fs::read_dir(&dir_path) {
		let mut inputs = vec![];
		let mut outputs = vec![];

		for entry in entries.flatten() {
			let file_name =
				entry.file_name().into_string().unwrap_or_default();
			if file_name.ends_with("_in.txt") {
				inputs.push(file_name);
			} else if file_name.ends_with("_out.txt") {
				outputs.push(file_name);
			}
		}

		inputs.sort();
		outputs.sort();

		// Pair inputs with corresponding outputs
		for input_file in inputs {
			let output_file = input_file.replace("_in.txt", "_out.txt");
			if outputs.contains(&output_file) {
				test_cases.push((input_file, output_file));
			}
		}
	}

	test_cases
}

#[test]
fn test_integration_standard() {
	let test_cases = collect_test_cases("standard");
	execute("standard", test_cases, true, "tb", vec![]);
}

#[test]
fn test_integration_income_statements() {
	let test_cases = collect_test_cases("incomestatement");
	execute("incomestatement", test_cases, true, "is", vec!["-i"]);
}

#[test]
fn test_integration_collapse_currency() {
	let test_cases = collect_test_cases("collapse");
	execute("collapse", test_cases, true, "tb", vec!["-c", "USD"]);
}

#[test]
fn test_integration_max_depth() {
	let test_cases = collect_test_cases("maxdepth");
	execute("maxdepth", test_cases, true, "bs", vec!["-d", "2"]);
}

#[test]
fn test_integration_low_precision() {
	let test_cases = collect_test_cases("precision");
	execute(
		"precision",
		test_cases,
		true,
		"bs",
		vec!["-p", "1", "-c", "USD"],
	);
}

#[test]
fn test_integration_clear() {
	let test_cases = collect_test_cases("clear");
	execute("clear", test_cases, true, "bs", vec![]);
}

#[test]
fn test_integration_should_fail() {
	let test_cases = collect_test_cases("failures");
	execute("failures", test_cases, false, "tb", vec![]);
}

#[test]
fn test_integration_account_summary() {
	let test_cases = collect_test_cases("acctsummary");
	execute("acctsummary", test_cases, true, "as", vec!["Assets:A"]);
}

#[test]
fn test_integration_exchange_rates() {
	let test_cases = collect_test_cases("exchangerates");
	execute("exchangerates", test_cases, true, "er", vec![]);
}

#[test]
fn test_integration_bounded_range() {
	let test_cases = collect_test_cases("bounded");
	execute(
		"bounded",
		test_cases,
		true,
		"tb",
		vec!["-b", "2024-11-13", "-e", "2024-11-17"],
	);
}

#[test]
fn test_integration_rgl() {
	let test_cases = collect_test_cases("rgl");
	execute("rgl", test_cases, true, "rgl", vec!["-e", "2024-11-23"]);
}

#[test]
fn test_integration_ugl() {
	let test_cases = collect_test_cases("ugl");
	execute("ugl", test_cases, true, "ugl", vec!["-e", "2024-11-23"]);
}

#[test]
fn test_integration_ioc() {
	let test_cases = collect_test_cases("ioc");
	execute(
		"ioc",
		test_cases,
		true,
		"bs",
		vec!["-c", "USD", "--ioc", "-E"],
	);
}

#[test]
fn test_integration_worthless() {
	let test_cases = collect_test_cases("worthless");
	execute(
		"worthless",
		test_cases,
		true,
		"bs",
		vec!["-c", "USD", "--ioc"],
	);
}

#[test]
fn test_integration_misc_flags() {
	let test_cases = collect_test_cases("miscflags");
	execute(
		"miscflags",
		test_cases,
		true,
		"bs",
		vec!["-i", "-E", "--lenient", "-p", "2"],
	);
}

#[test]
fn test_integration_find() {
	let test_cases = collect_test_cases("find");
	execute("find", test_cases, true, "find", vec!["Needle"]);
}

#[test]
fn test_integration_check() {
	let test_cases = collect_test_cases("check");
	execute("check", test_cases, true, "check", vec![]);
}

#[test]
fn test_integration_canonical_fmt() {
	let test_cases = collect_test_cases("canon");
	execute("canon", test_cases, true, "fmt", vec!["-b", "2024-11-10"]);
}

fn execute(
	subfolder: &str,
	test_cases: Vec<(String, String)>,
	should_succeed: bool,
	cmd: &str,
	args: Vec<&str>,
) {
	for (input_file, expected_output_file) in test_cases {
		println!("running for {}...", input_file);

		let loc = format!("{}/{}/{}", "tests/test_data", subfolder, input_file);

		let all_args =
			[vec!["run", "--", "-f", loc.as_str(), cmd], args.clone()].concat();

		let output = Command::new("cargo")
			.args(all_args)
			.output()
			.expect("Failed to execute process");

		if !should_succeed {
			assert!(
				!output.status.success(),
				"{} unexpectedly succeeded!",
				input_file
			);
			continue;
		}

		assert!(
			output.status.success(),
			"{} failed processing: {}",
			input_file,
			String::from_utf8_lossy(&output.stderr)
		);

		let stdout = String::from_utf8_lossy(&output.stdout);

		let expected_output = fs::read_to_string(format!(
			"{}/{}/{}",
			"tests/test_data", subfolder, expected_output_file
		))
		.expect("Failed to read expected output file");

		assert_eq!(
			stdout.trim(),
			expected_output.trim(),
			"Output did not match for {}; expected:\n{}\ngot:\n{}",
			input_file,
			expected_output.trim(),
			stdout.trim()
		);
	}
}