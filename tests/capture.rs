//! `screenshot` on the real screen. Opt-in with `BIGARROW_SCREEN_TESTS=1`, because these read the
//! screen of whoever runs them and draw an arrow on it.

use std::path::PathBuf;
use std::process::Command;
use std::sync::Mutex;
use std::time::Duration;

use tiny_skia::Pixmap;

/// Arrows share the screen: one test at a time.
static SCREEN: Mutex<()> = Mutex::new(());

fn enabled() -> bool {
    let on = std::env::var("BIGARROW_SCREEN_TESTS").is_ok_and(|value| value == "1");
    if !on {
        eprintln!("skipped: set BIGARROW_SCREEN_TESTS=1 to read and draw on the screen");
    }
    on
}

fn bigarrow(args: &[&str]) -> (i32, serde_json::Value) {
    let output = Command::new(env!("CARGO_BIN_EXE_bigarrow"))
        .args(args)
        .output()
        .expect("bigarrow runs");
    let text = String::from_utf8_lossy(if output.stdout.is_empty() {
        &output.stderr
    } else {
        &output.stdout
    })
    .into_owned();
    let json = serde_json::from_str(text.trim()).unwrap_or(serde_json::Value::Null);
    (output.status.code().unwrap_or(-1), json)
}

/// Display 1's top-left corner and size.
fn display() -> [i64; 4] {
    let (_, doctor) = bigarrow(&["doctor", "--json"]);
    let display = &doctor["displays"][0];
    ["x", "y", "width", "height"].map(|key| display[key].as_i64().unwrap())
}

fn temp(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "bigarrow-capture-{}-{name}.png",
        std::process::id()
    ))
}

#[test]
fn a_rect_comes_back_pixel_for_pixel_and_shrinks_on_request() {
    let _screen = SCREEN.lock().unwrap();
    if !enabled() {
        return;
    }
    let [x, y, ..] = display();
    let rect = format!("{},{},64,48", x + 100, y + 100);
    let file = temp("rect");
    let png = file.to_str().unwrap();
    let (code, json) = bigarrow(&["screenshot", "--rect", &rect, "--png", png, "--json"]);
    assert_eq!(code, 0, "{json}");
    assert_eq!(json["rect"], serde_json::json!([x + 100, y + 100, 64, 48]));
    assert_eq!(json["size"], serde_json::json!([64, 48]));
    let path = PathBuf::from(json["path"].as_str().unwrap());
    assert!(path.is_absolute(), "{path:?}");
    let image = Pixmap::load_png(&path).unwrap();
    assert_eq!((image.width(), image.height()), (64, 48));

    let (code, json) = bigarrow(&[
        "screenshot",
        "--rect",
        &rect,
        "--png",
        png,
        "--max-size",
        "32",
        "--json",
    ]);
    assert_eq!(code, 0, "{json}");
    assert_eq!(json["size"], serde_json::json!([32, 24]));
    let _ = std::fs::remove_file(file);
}

#[test]
fn an_arrow_is_where_its_sign_says() {
    let _screen = SCREEN.lock().unwrap();
    if !enabled() {
        return;
    }
    let [x, y, width, height] = display();
    let at = format!("{},{}", x + width * 45 / 100, y + height * 45 / 100);
    let (code, arrow) = bigarrow(&[
        "start",
        "--at",
        &at,
        "--color",
        "green",
        "--text",
        "Capture test",
        "--no-animation",
        "--duration",
        "0",
        "--json",
    ]);
    assert_eq!(code, 0, "{arrow}");
    let pid = arrow["pid"].as_u64().unwrap().to_string();
    let sign: Vec<String> = arrow["sign"]
        .as_array()
        .unwrap()
        .iter()
        .map(|value| value.as_i64().unwrap().to_string())
        .collect();
    // The first frame is up when `start` returns; give the compositor a few frames.
    std::thread::sleep(Duration::from_millis(300));
    let file = temp("sign");
    let (code, shot) = bigarrow(&[
        "screenshot",
        "--rect",
        &sign.join(","),
        "--png",
        file.to_str().unwrap(),
        "--json",
    ]);
    bigarrow(&["clear", "--pid", &pid]);
    assert_eq!(code, 0, "{shot}");
    let image = Pixmap::load_png(&file).unwrap();
    let _ = std::fs::remove_file(file);
    // The green preset, #21B24B, fills most of the sign around its white text.
    let green = image
        .pixels()
        .iter()
        .map(|pixel| pixel.demultiply())
        .filter(|pixel| {
            pixel.red().abs_diff(0x21) < 24
                && pixel.green().abs_diff(0xB2) < 24
                && pixel.blue().abs_diff(0x4B) < 24
        })
        .count();
    let all = image.pixels().len();
    assert!(
        green * 10 > all * 4,
        "{green} of {all} pixels in the sign are the arrow's green"
    );
}
