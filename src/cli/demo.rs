use super::options::DemoArgs;
use super::output::number;
use crate::model::error::{Error, Result};
use crate::platform;

/// `demo`: an arrow, a ring at the mouse and a box, one after the other, on the display under the mouse.
pub fn run(args: &DemoArgs) -> Result<()> {
    let screens = platform::screens()?;
    let mouse = platform::mouse_location().ok();
    let display = mouse
        .and_then(|mouse| screens.containing(mouse).ok())
        .or(screens.displays.first())
        .ok_or_else(|| Error::bad_input("no display"))?;
    let frame = display.visible;
    let center = format!("{},{}", frame.mid_x() as i64, frame.mid_y() as i64);
    let rect = format!(
        "{},{},{},{}",
        (frame.x + frame.width * 0.62) as i64,
        (frame.y + frame.height * 0.62) as i64,
        (220.0 * display.scale) as i64,
        (90.0 * display.scale) as i64
    );
    let seconds = number(args.step);
    let steps: [Vec<&str>; 3] = [
        vec![
            "point",
            "--at",
            &center,
            "--text",
            "bigarrow points at things",
            "--duration",
            &seconds,
        ],
        vec![
            "point",
            "--mouse",
            "--style",
            "ring",
            "--color",
            "blue",
            "--text",
            "You are here",
            "--duration",
            &seconds,
        ],
        vec![
            "point",
            "--rect",
            &rect,
            "--color",
            "green",
            "--text",
            "Type your name here",
            "--duration",
            &seconds,
        ],
    ];
    let executable = std::env::current_exe()
        .map_err(|_| Error::bad_input("cannot find the bigarrow executable"))?;
    for step in steps {
        let status = std::process::Command::new(&executable).args(&step).status();
        if !status.is_ok_and(|status| status.success()) {
            return Err(Error::unresolvable(format!(
                "demo step failed: bigarrow {}",
                step.join(" ")
            )));
        }
    }
    Ok(())
}
