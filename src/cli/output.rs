//! Every line the CLI writes goes through here: results to stdout, errors to stderr. Writes never
//! panic: a detached arrow's parent may already be gone.

use std::io::Write;

use serde::Serialize;

use crate::model::error::Error;
use crate::model::result::ErrorResult;

pub fn print(line: &str) {
    let mut stdout = std::io::stdout().lock();
    let _ = writeln!(stdout, "{line}");
    let _ = stdout.flush();
}

pub fn print_error(line: &str) {
    let _ = writeln!(std::io::stderr().lock(), "{line}");
}

pub fn json<T: Serialize>(value: &T) {
    print(&serde_json::to_string(value).expect("results always serialize"));
}

/// One line on stderr (JSON with `--json`), then the error's exit code.
pub fn fail(error: &Error, json: bool) -> ! {
    let line = error.message.replace('\n', " ");
    if json {
        let result = ErrorResult {
            code: error.code,
            error: line,
            ok: false,
        };
        print_error(&serde_json::to_string(&result).expect("errors always serialize"));
    } else {
        print_error(&format!("bigarrow: {line}"));
    }
    std::process::exit(error.code)
}

/// `12` for whole numbers, `12.5` otherwise.
pub fn number(value: f64) -> String {
    if value == value.round() {
        format!("{}", value as i64)
    } else {
        format!("{value:.1}")
    }
}
