//! Integration test for `mandelbot --headless`.
//!
//! Runs the checked-in demo scenario through the real binary and asserts on
//! the structure of the resulting newline-delimited JSON snapshots. This
//! guards against regressions in both the headless driver itself and the
//! pieces of `App::update` that it exercises (tab spawn, focus fallback on
//! close, title/status plumbing).

use std::process::Command;

use serde_json::Value;

fn run_scenario(scenario: &str) -> String {
    let bin = env!("CARGO_BIN_EXE_mandelbot");
    let output = Command::new(bin)
        .args(["--headless", scenario])
        .output()
        .expect("failed to spawn mandelbot");
    assert!(
        output.status.success(),
        "mandelbot --headless {scenario} exited with {:?}\nstderr: {}",
        output.status,
        String::from_utf8_lossy(&output.stderr),
    );
    String::from_utf8(output.stdout).expect("stdout not utf-8")
}

fn parse_snapshots(stdout: &str) -> Vec<Value> {
    stdout
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| serde_json::from_str::<Value>(l).expect("each line is a JSON object"))
        .collect()
}

#[test]
fn demo_scenario_produces_expected_snapshots() {
    let stdout = run_scenario("examples/headless-demo.json");
    let snapshots = parse_snapshots(&stdout);
    assert_eq!(snapshots.len(), 2, "demo scenario emits two snapshots");

    let first = &snapshots[0];
    assert_eq!(first["label"], "after-two-shells");
    assert_eq!(first["active_tab_id"], 2);
    let first_tabs = first["tabs"].as_array().expect("tabs is an array");
    assert_eq!(first_tabs.len(), 3, "home + 2 shells");
    assert_eq!(first_tabs[0]["title"], "demo-home");
    assert_eq!(first_tabs[0]["is_claude"], true);
    assert_eq!(first_tabs[1]["is_claude"], false);
    assert_eq!(first_tabs[2]["is_claude"], false);

    let second = &snapshots[1];
    assert_eq!(second["label"], "after-close");
    // Closing tab 2 (the active shell) should fall focus back to its neighbor.
    assert_eq!(second["active_tab_id"], 1);
    let second_tabs = second["tabs"].as_array().expect("tabs is an array");
    assert_eq!(second_tabs.len(), 2, "home + 1 shell after close");
    let ids: Vec<u64> = second_tabs
        .iter()
        .map(|t| t["id"].as_u64().unwrap())
        .collect();
    assert_eq!(ids, vec![0, 1]);
}

#[test]
fn set_status_unknown_value_reports_error() {
    let dir = tempdir();
    let path = dir.join("bad-status.json");
    std::fs::write(
        &path,
        r#"{
            "actions": [
                { "SetStatus": { "tab_id": 0, "status": "not_a_real_status" } }
            ]
        }"#,
    )
    .unwrap();

    let bin = env!("CARGO_BIN_EXE_mandelbot");
    let output = Command::new(bin)
        .args(["--headless", path.to_str().unwrap()])
        .output()
        .expect("failed to spawn mandelbot");

    assert!(!output.status.success(), "expected non-zero exit");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("unknown SetStatus value"),
        "stderr should name the bad status: {stderr}"
    );
}

fn tempdir() -> std::path::PathBuf {
    let mut p = std::env::temp_dir();
    let pid = std::process::id();
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    p.push(format!("mandelbot-headless-test-{pid}-{nanos}"));
    std::fs::create_dir_all(&p).unwrap();
    p
}
