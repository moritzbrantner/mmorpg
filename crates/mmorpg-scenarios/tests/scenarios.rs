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
use std::process::Command;

use mmorpg_scenarios::{Report, bots, control_plane};
use serde_json::Value;

type Runner = fn(&str) -> Result<Report, String>;

fn run_bots(text: &str) -> Result<Report, String> {
    bots::run(&bots::load(text)?)
}

fn run_control_plane(text: &str) -> Result<Report, String> {
    control_plane::run(&control_plane::load(text)?)
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
fn control_plane_scenarios_match_expected_output() {
    check_goldens("control-plane", run_control_plane);
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
forward = 1
strafe = 0
facing = 16384
seq = 0
[[expect]]
kind = "position"
bot = "alice"
tick = 2
position = [999, 90, 0]
"#,
    )
    .unwrap();
    let text = report.to_text();
    assert!(!report.passed());
    assert_eq!(report.failures(), 2, "{text}");
    assert!(text.contains("FAIL expected applied"), "{text}");
    assert!(text.contains("got [0, 90, 0]"), "{text}");
    assert!(text.ends_with("failures=2\n"), "{text}");
}

#[test]
fn area_expectations_compare_with_the_core_area_table() {
    let report = run_bots(
        r#"
name = "wrong-area"
zone = 1
ticks = 1
[[bots]]
name = "alice"
[[steps]]
tick = 0
bot = "alice"
action = "join"
[[steps]]
tick = 0
bot = "alice"
action = "jump"
seq = 3
[[expect]]
kind = "area"
bot = "alice"
tick = 1
area = "Nowhere"
"#,
    )
    .unwrap();
    let text = report.to_text();
    assert!(!report.passed());
    assert!(text.contains("jump -> applied seq=3"), "{text}");
    assert!(text.contains("FAIL alice in Greyhaven Outpost"), "{text}");
}

#[test]
fn unexpected_control_plane_outcomes_fail_the_scenario() {
    let report = run_control_plane(
        r#"
name = "wrong"
lease_ttl_ticks = 10
heartbeat_ttl_ticks = 10
[[steps]]
at = 0
op = "assign"
zone = 1
host = "host-a"
"#,
    )
    .unwrap();
    assert!(!report.passed());
    assert!(report.to_text().contains("-> rejected:host_not_registered"));
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
            "forward, strafe and facing",
        ),
        (
            "name = \"x\"\nzone = 1\nticks = 2\n[[bots]]\nname = \"a\"\n[[steps]]\ntick = 0\nbot = \"a\"\naction = \"move\"\nforward = 1\nstrafe = 0",
            "forward, strafe and facing",
        ),
        (
            "name = \"x\"\nzone = 1\nticks = 2\n[[bots]]\nname = \"a\"\n[[steps]]\ntick = 0\nbot = \"a\"\naction = \"join\"\nfacing = 0",
            "forward, strafe and facing",
        ),
        (
            "name = \"x\"\nzone = 1\nticks = 2\n[[bots]]\nname = \"a\"\n[[steps]]\ntick = 0\nbot = \"a\"\naction = \"move\"\nx = 1\nz = 0",
            "unknown field",
        ),
        (
            "name = \"x\"\nzone = 1\nticks = 2\ntypo = 1\n[[bots]]\nname = \"a\"",
            "unknown field",
        ),
        (
            "name = \"x\"\nzone = 1\nticks = 2\n[[bots]]\nname = \"a\"\n[[steps]]\ntick = 0\nbot = \"a\"\naction = \"jump\"\nfacing = 0",
            "forward, strafe and facing",
        ),
        (
            "name = \"x\"\nzone = 1\nticks = 2\n[[bots]]\nname = \"a\"\n[[steps]]\ntick = 0\nbot = \"a\"\naction = \"disconnect\"\nseq = 2",
            "apply only to move and jump",
        ),
        (
            "name = \"x\"\nzone = 1\nticks = 2\n[[bots]]\nname = \"a\"\n[[expect]]\nkind = \"area\"\nbot = \"a\"\ntick = 1",
            "missing its required field",
        ),
    ] {
        let error = bots::load(text).unwrap_err();
        assert!(error.contains(message), "{error}");
    }
    let error = control_plane::load(
        "name = \"x\"\nlease_ttl_ticks = 1\nheartbeat_ttl_ticks = 1\n[[steps]]\nat = 5\nop = \"expire_hosts\"\n[[steps]]\nat = 4\nop = \"expire_hosts\"",
    )
    .unwrap_err();
    assert!(error.contains("backwards"), "{error}");
}

fn cli(arguments: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_mmorpg-scenario"))
        .args(arguments)
        .output()
        .unwrap()
}

#[test]
fn cli_reports_pass_fail_and_usage_through_exit_codes() {
    let passing = scenario_files("control-plane")[0].clone();
    let passing = passing.to_str().unwrap();
    let output = cli(&["control-plane", "--json", passing]);
    assert_eq!(output.status.code(), Some(0));
    let expected = run_control_plane(&std::fs::read_to_string(passing).unwrap())
        .unwrap()
        .to_json_lines();
    assert_eq!(String::from_utf8(output.stdout).unwrap(), expected);

    let failing = Path::new(env!("CARGO_TARGET_TMPDIR")).join("failing-scenario.toml");
    std::fs::write(
        &failing,
        "name = \"f\"\nlease_ttl_ticks = 1\nheartbeat_ttl_ticks = 1\n[[steps]]\nat = 0\nop = \"heartbeat\"\nhost = \"h\"\n",
    )
    .unwrap();
    let output = cli(&["control-plane", failing.to_str().unwrap()]);
    assert_eq!(output.status.code(), Some(1));

    assert_eq!(cli(&["bots", "missing.toml"]).status.code(), Some(2));
    assert_eq!(cli(&["nonsense", passing]).status.code(), Some(2));
    assert_eq!(cli(&[]).status.code(), Some(2));
}
