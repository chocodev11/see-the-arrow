//! The CLI contract through the real binary, drawing nothing (`--dry-run`, `--png`).

use std::io::Write;
use std::process::{Command, Stdio};

struct Run {
    code: i32,
    stdout: String,
    stderr: String,
}

impl Run {
    fn json(&self) -> serde_json::Value {
        let text = if self.stdout.trim().is_empty() {
            &self.stderr
        } else {
            &self.stdout
        };
        serde_json::from_str(text.trim()).unwrap_or_else(|_| panic!("not JSON: {text}"))
    }
}

fn run(args: &[&str]) -> Run {
    run_with(args, None)
}

fn run_with(args: &[&str], stdin: Option<&str>) -> Run {
    let mut child = Command::new(env!("CARGO_BIN_EXE_bigarrow"))
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("bigarrow runs");
    let mut input = child.stdin.take().expect("stdin is piped");
    input
        .write_all(stdin.unwrap_or_default().as_bytes())
        .expect("stdin takes the input");
    drop(input);
    let output = child.wait_with_output().expect("bigarrow ends");
    Run {
        code: output.status.code().unwrap_or(-1),
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
    }
}

/// A point well inside the primary display.
fn inside() -> String {
    let doctor = run(&["doctor", "--json"]).json();
    let display = &doctor["displays"][0];
    let x = display["x"].as_f64().unwrap() + display["width"].as_f64().unwrap() * 0.45;
    let y = display["y"].as_f64().unwrap() + display["height"].as_f64().unwrap() * 0.45;
    assert!(
        display["x"].is_i64(),
        "display coordinates are whole pixels"
    );
    format!("{},{}", x as i64, y as i64)
}

#[test]
fn version_and_bare_command() {
    let version = run(&["--version"]);
    assert_eq!(version.code, 0);
    assert!(version.stdout.starts_with("bigarrow "));
    let bare = run(&[]);
    assert_eq!(bare.code, 0);
    assert!(bare.stdout.contains("Usage"));
}

#[test]
fn a_point_outside_every_display_is_bad_input_on_one_line() {
    let result = run(&["point", "--at", "99999,0", "--text", "x"]);
    assert_eq!(result.code, 2);
    assert!(result.stderr.contains("point is outside every display"));
    assert_eq!(result.stderr.trim_end().lines().count(), 1);
}

#[test]
fn errors_are_json_on_stderr_with_json() {
    let result = run(&[
        "point", "--at", "1,2", "--text", "x", "--color", "rainbow", "--json",
    ]);
    assert_eq!(result.code, 2);
    let json = result.json();
    assert_eq!(json["ok"], false);
    assert_eq!(json["code"], 2);
    let parser = run(&["point", "--text", "x", "--bogus", "--json"]);
    assert_eq!(parser.code, 2);
    assert_eq!(parser.json()["code"], 2);
}

#[test]
fn parser_errors_are_bad_input() {
    assert_eq!(run(&["point", "--text", "x", "--bogus"]).code, 2);
    assert_eq!(run(&["point", "--at", "1,2"]).code, 2);
    assert_eq!(
        run(&["point", "--at", "1,2", "--mouse", "--text", "x"]).code,
        2
    );
    assert_eq!(
        run(&["point", "--at", "1,2", "--text", "x", "--duration", "-1"]).code,
        2
    );
    assert_eq!(
        run(&["point", "--mouse", "--text", "x", "--follow", "--dry-run"]).code,
        2
    );
}

#[test]
fn a_repeated_option_takes_the_last_value() {
    let result = run(&[
        "point",
        "--at",
        &inside(),
        "--text",
        "x",
        "--color",
        "rainbow",
        "--color",
        "blue",
        "--dry-run",
    ]);
    assert_eq!(result.code, 0, "{}", result.stderr);
}

#[test]
fn display_relative_coordinates_and_rects() {
    let json = run(&[
        "point",
        "--at",
        "100,100",
        "--display",
        "1",
        "--text",
        "x",
        "--dry-run",
        "--json",
    ])
    .json();
    assert_eq!(json["target"]["display"], 1);
    assert_eq!(json["dryRun"], true);
    assert_eq!(
        run(&["point", "--at", "1,1", "--display", "99", "--text", "x"]).code,
        2
    );
    assert_eq!(
        run(&["point", "--mouse", "--display", "1", "--text", "x"]).code,
        2
    );
    let doctor = run(&["doctor", "--json"]).json();
    let display = &doctor["displays"][0];
    let (x, y) = (
        display["x"].as_i64().unwrap() + 300,
        display["y"].as_i64().unwrap() + 300,
    );
    let rect = format!("{x},{y},200,80");
    let json = run(&[
        "point",
        "--rect",
        &rect,
        "--text",
        "x",
        "--dry-run",
        "--json",
    ])
    .json();
    assert_eq!(
        json["target"]["rect"],
        serde_json::json!([x as f64, y as f64, 200.0, 80.0])
    );
    assert_eq!(json["target"]["source"], "rect");
}

#[test]
fn an_unknown_app_exits_3_and_names_apps() {
    let result = run(&["point", "--window", "No Such App 4711", "--text", "x"]);
    assert_eq!(result.code, 3);
    assert!(result.stderr.contains("apps with windows"));
}

#[test]
fn window_and_app_conflict_but_app_goes_with_coordinates() {
    let result = run(&[
        "point",
        "--window",
        "Notepad",
        "--app",
        "Notepad",
        "--text",
        "x",
        "--dry-run",
    ]);
    assert_eq!(result.code, 2);
    assert!(result.stderr.contains("drop --app"));
    let result = run(&[
        "point",
        "--at",
        &inside(),
        "--app",
        "Notepad",
        "--no-raise",
        "--text",
        "x",
        "--dry-run",
    ]);
    assert_eq!(result.code, 0, "{}", result.stderr);
}

#[test]
fn look_options_are_accepted_and_checked() {
    let at = inside();
    let base = ["point", "--at", at.as_str(), "--text", "x", "--dry-run"];
    let colours = [
        "--border-color",
        "red",
        "--text-color",
        "#00FF00",
        "--edge-color",
        "purple",
        "--close-color",
        "black",
        "--close-x-color",
        "white",
        "--close-button",
    ];
    for border in ["shadow", "white-black", "black"] {
        let args: Vec<&str> = base
            .iter()
            .copied()
            .chain(["--border", border])
            .chain(colours)
            .collect();
        assert_eq!(run(&args).code, 0, "--border {border}");
    }
    for (flag, value) in [
        ("--border", "glow"),
        ("--text-color", "rainbow"),
        ("--shape", "wave"),
        ("--size", "XL"),
        ("--from", "north"),
    ] {
        let args: Vec<&str> = base.iter().copied().chain([flag, value]).collect();
        assert_eq!(run(&args).code, 2, "{flag} {value}");
    }
}

#[test]
fn stop_hook_is_silent() {
    let result = run_with(
        &["stop", "--hook"],
        Some(r#"{"session_id":"no-such-session","prompt":"hi"}"#),
    );
    assert_eq!(result.code, 0);
    assert!(result.stdout.is_empty());
    assert_eq!(
        run_with(&["stop", "--hook", "--json"], Some("not json")).code,
        0
    );
    let missing = run(&["stop", "--pid", "999999"]);
    assert_eq!(missing.code, 3);
}

#[test]
fn doctor_reports_displays_and_needs_no_permission_to_draw() {
    let json = run(&["doctor", "--json"]).json();
    assert!(!json["displays"].as_array().unwrap().is_empty());
    assert_eq!(json["drawingNeedsPermission"], false);
    assert_eq!(json["ok"], true);
    assert_eq!(run(&["doctor", "--require", "teleport"]).code, 2);
}

#[test]
fn copy_buttons_lie_in_the_sign_and_braces_must_balance() {
    let json = run(&[
        "point",
        "--at",
        &inside(),
        "--text",
        "Copy {{abc}} or {{def}}",
        "--dry-run",
        "--json",
    ])
    .json();
    let sign: Vec<i64> = json["sign"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_i64().unwrap())
        .collect();
    let chips = json["copyButtons"].as_array().unwrap();
    assert_eq!(chips.len(), 2);
    for chip in chips {
        let chip: Vec<i64> = chip
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_i64().unwrap())
            .collect();
        assert!(
            chip[0] >= sign[0]
                && chip[1] >= sign[1]
                && chip[0] + chip[2] <= sign[0] + sign[2]
                && chip[1] + chip[3] <= sign[1] + sign[3]
        );
    }
    let bad = run(&[
        "point",
        "--at",
        &inside(),
        "--text",
        "Copy {{abc",
        "--dry-run",
    ]);
    assert_eq!(bad.code, 2);
    assert!(bad.stderr.contains("{{value}}"));
}

#[test]
fn png_renders_offscreen() {
    let file = std::env::temp_dir().join(format!("bigarrow-test-{}.png", std::process::id()));
    let result = run(&[
        "point",
        "--at",
        &inside(),
        "--text",
        "Franz, click HERE",
        "--png",
        file.to_str().unwrap(),
        "--json",
    ]);
    assert_eq!(result.code, 0, "{}", result.stderr);
    let json = result.json();
    assert_eq!(json["image"].as_array().unwrap().len(), 3);
    let data = std::fs::read(&file).unwrap();
    assert_eq!(&data[1..4], b"PNG");
    let _ = std::fs::remove_file(file);
}

#[test]
fn screenshot_arguments_are_checked_before_the_screen_is_read() {
    let file = std::env::temp_dir().join(format!("bigarrow-test-shot-{}.png", std::process::id()));
    let png = file.to_str().unwrap();
    assert_eq!(run(&["screenshot"]).code, 2);
    for args in [
        ["--window", "Notepad", "--rect", "0,0,10,10"],
        ["--window", "Notepad", "--display", "1"],
        ["--max-size", "0", "--display", "1"],
        ["--rect", "99999,99999,10,10", "--display", "1"],
    ] {
        let result = run(&[&["screenshot", "--png", png], &args[..]].concat());
        assert_eq!(result.code, 2, "{args:?}: {}", result.stderr);
    }
    assert!(!file.exists());
}
