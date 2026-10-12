use super::options::ElementsArgs;
use super::{output, targeting};
use crate::model::error::Result;
use crate::platform;

/// `elements`: the visible, labelled UI elements of an app that `--element` can point at.
pub fn run(args: &ElementsArgs) -> Result<()> {
    let screens = platform::screens()?;
    let nodes = targeting::list_elements(
        args.app.as_deref(),
        args.role.as_deref(),
        args.matching.as_deref(),
        &screens,
    )?;
    if args.json {
        output::json(&nodes);
        return Ok(());
    }
    for node in &nodes {
        let frame = node.frame.map_or("-".to_owned(), targeting::rect_text);
        let label: Vec<&str> = [&node.name, &node.value]
            .into_iter()
            .flatten()
            .map(String::as_str)
            .filter(|text| !text.is_empty())
            .collect();
        output::print(&format!("{frame}  {}  {}", node.role, label.join(" | ")));
    }
    Ok(())
}
