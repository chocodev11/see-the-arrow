use super::options::FrontArgs;
use super::{output, targeting};
use crate::model::error::{Error, Result};
use crate::platform;

/// `front`: brings an app, and optionally one of its windows or tabs, to the front.
pub fn run(args: &FrontArgs) -> Result<()> {
    let screens = platform::screens()?;
    let raised = targeting::raise(&args.app, args.window.as_deref(), &screens)?;
    if !raised.frontmost {
        platform::flash(raised.handle);
        return Err(Error::unresolvable(format!(
            "{} did not come to the front within 1.5 s",
            raised.app
        )));
    }
    if args.json {
        output::json(&raised);
    } else {
        let window = raised
            .window
            .as_ref()
            .map(|window| format!(", window '{window}' raised"))
            .unwrap_or_default();
        output::print(&format!("bigarrow: {} is in front{window}", raised.app));
    }
    Ok(())
}
