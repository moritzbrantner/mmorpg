#![forbid(unsafe_code)]
//! `mmorpg-scenario <bots|control-plane> [--json] <scenario.toml>...`
//!
//! Exit status: 0 when every scenario passes, 1 when any scenario fails,
//! 2 for usage, file or scenario-format errors.

use std::io::Write;
use std::process::ExitCode;

use mmorpg_scenarios::{Report, bots, control_plane};

const USAGE: &str = "usage: mmorpg-scenario <bots|control-plane> [--json] <scenario.toml>...";

fn main() -> ExitCode {
    let mut arguments = std::env::args().skip(1);
    let Some(tool) = arguments.next() else {
        eprintln!("{USAGE}");
        return ExitCode::from(2);
    };
    let mut json = false;
    let mut paths = Vec::new();
    for argument in arguments {
        match argument.as_str() {
            "--json" => json = true,
            "-h" | "--help" => {
                println!("{USAGE}");
                return ExitCode::SUCCESS;
            }
            flag if flag.starts_with('-') => {
                eprintln!("unknown flag {flag}\n{USAGE}");
                return ExitCode::from(2);
            }
            _ => paths.push(argument),
        }
    }
    if paths.is_empty() {
        eprintln!("{USAGE}");
        return ExitCode::from(2);
    }
    let run: fn(&str) -> Result<Report, String> = match tool.as_str() {
        "bots" => |text| bots::run(&bots::load(text)?),
        "control-plane" => |text| control_plane::run(&control_plane::load(text)?),
        other => {
            eprintln!("unknown tool {other}\n{USAGE}");
            return ExitCode::from(2);
        }
    };
    let mut all_passed = true;
    let mut stdout = std::io::stdout().lock();
    for path in paths {
        let report = std::fs::read_to_string(&path)
            .map_err(|error| error.to_string())
            .and_then(|text| run(&text));
        let report = match report {
            Ok(report) => report,
            Err(error) => {
                eprintln!("{path}: {error}");
                return ExitCode::from(2);
            }
        };
        all_passed &= report.passed();
        let output = if json {
            report.to_json_lines()
        } else {
            report.to_text()
        };
        if stdout.write_all(output.as_bytes()).is_err() {
            return ExitCode::from(2);
        }
    }
    if all_passed {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}
