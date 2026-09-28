//! Checked-in scenarios and their expected outputs.
//!
//! Every `scenarios/<tool>/<name>.toml` must pass and reproduce
//! `<name>.expected` byte for byte. After an intentional behavior change,
//! regenerate with:
//!
//! ```sh
//! MMORPG_SCENARIOS_UPDATE=1 cargo test -p mmorpg-scenarios --test scenarios --locked
//! ```

use std::path::{Path, PathBuf};

use mmorpg_scenarios::{Report, bots};
use serde_json::Value;

type Runner = fn(&str) -> Result<Report, String>;

fn run_bots(text: &str) -> Result<Report, String> {
    bots::run(&bots::load(text)?)
}

fn scenario_files(tool: &str) -> Vec<PathBuf> {
    let directory = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("scenarios")
        .join(tool);
    let mut files = std::fs::read_dir(&directory)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| {
            path.extension()
                .is_some_and(|extension| extension == "toml")
        })
        .collect::<Vec<_>>();
    files.sort();
    assert!(!files.is_empty(), "no scenarios in {}", directory.display());
    files
}

fn check_goldens(tool: &str, run: Runner) {
    let update = std::env::var_os("MMORPG_SCENARIOS_UPDATE").is_some();
    let mut mismatches = Vec::new();
    for path in scenario_files(tool) {
        let text = std::fs::read_to_string(&path).unwrap();
        let report = run(&text).unwrap_or_else(|error| panic!("{}: {error}", path.display()));
        let actual = report.to_text();
        let expected_path = path.with_extension("expected");
        if update {
            std::fs::write(&expected_path, &actual).unwrap();
        } else {
            let expected = std::fs::read_to_string(&expected_path).unwrap_or_default();
            if expected != actual {
                mismatches.push(format!(
                    "{}\n--- expected\n{expected}--- actual\n{actual}",
                    path.display()
                ));
            }
        }
        assert!(
            report.passed(),
            "checked-in scenario {} must pass:\n{actual}",
            path.display()
        );
        check_json_matches_text(&report);
    }
    assert!(
        mismatches.is_empty(),
        "scenario output changed; rerun with MMORPG_SCENARIOS_UPDATE=1 if intentional:\n{}",
        mismatches.join("\n")
    );
}

fn check_json_matches_text(report: &Report) {
    let json = report.to_json_lines();
    let lines = json
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).unwrap())
        .collect::<Vec<_>>();
    assert_eq!(lines.len(), report.to_text().lines().count());
    let result = lines.last().unwrap();
    assert_eq!(result["type"], "result");
    assert_eq!(result["passed"], report.passed());
}

#[test]
fn bot_scenarios_match_expected_output() {
    check_goldens("bots", run_bots);
}

#[test]
fn bot_runs_are_deterministic() {
    for path in scenario_files("bots") {
        let text = std::fs::read_to_string(&path).unwrap();
        let first = run_bots(&text).unwrap().to_json_lines();
        let second = run_bots(&text).unwrap().to_json_lines();
        assert_eq!(first, second, "{}", path.display());
    }
}

#[test]
fn unmet_bot_expectations_fail_the_scenario() {
    let report = run_bots(
        r#"
name = "wrong"
zone = 1
ticks = 2
[[bots]]
name = "alice"
[[steps]]
tick = 0
bot = "alice"
action = "join"
[[steps]]
tick = 1
bot = "alice"
action = "move"
x = 1
z = 0
seq = 0
[[expect]]
kind = "position"
bot = "alice"
tick = 2
position = [999, 50, 0]
"#,
    )
    .unwrap();
    let text = report.to_text();
    assert!(!report.passed());
    assert_eq!(report.failures(), 2, "{text}");
    assert!(text.contains("FAIL expected applied"), "{text}");
    assert!(text.contains("got [0, 50, 0]"), "{text}");
    assert!(text.ends_with("failures=2\n"), "{text}");
}

#[test]
fn invalid_scenarios_are_rejected_at_load() {
    for (text, message) in [
        (
            "name = \"x\"\nzone = 1\nticks = 0\n[[bots]]\nname = \"a\"",
            "ticks",
        ),
        ("name = \"x\"\nzone = 1\nticks = 2\nbots = []", "bots"),
        (
            "name = \"x\"\nzone = 1\nticks = 2\n[[bots]]\nname = \"a\"\n[[steps]]\ntick = 0\nbot = \"b\"\naction = \"join\"",
            "unknown bot",
        ),
        (
            "name = \"x\"\nzone = 1\nticks = 2\n[[bots]]\nname = \"a\"\n[[steps]]\ntick = 0\nbot = \"a\"\naction = \"move\"",
            "x and z",
        ),
        (
            "name = \"x\"\nzone = 1\nticks = 2\ntypo = 1\n[[bots]]\nname = \"a\"",
            "unknown field",
        ),
    ] {
        let error = bots::load(text).unwrap_err();
        assert!(error.contains(message), "{error}");
    }
}
