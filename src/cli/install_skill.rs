//! `install-skill`: copies the big-arrow skill to Claude Code (`~/.claude/skills/big-arrow`) and
//! Codex (`~/.agents/skills/big-arrow`). Idempotent; refuses to overwrite changed files unless --force.

use std::fs;
use std::path::{Path, PathBuf};

use super::options::InstallSkillArgs;
use super::output;
use crate::model::error::{Error, Result};

/// The skill shipped inside the binary, relative path and content.
const SHIPPED: [(&str, &str); 3] = [
    ("SKILL.md", include_str!("../../skill/big-arrow/SKILL.md")),
    (
        "reference.md",
        include_str!("../../skill/big-arrow/reference.md"),
    ),
    (
        "agents/openai.yaml",
        include_str!("../../skill/big-arrow/agents/openai.yaml"),
    ),
];

pub fn run(args: &InstallSkillArgs) -> Result<()> {
    let files = match &args.source {
        Some(folder) => read_folder(Path::new(folder))?,
        None => SHIPPED
            .iter()
            .map(|(path, text)| ((*path).to_owned(), text.as_bytes().to_vec()))
            .collect(),
    };
    let home =
        std::env::home_dir().ok_or_else(|| Error::bad_input("cannot find your home folder"))?;
    let folder = |agent: &str| home.join(agent).join("skills").join("big-arrow");
    let agents: Vec<(&str, PathBuf)> = [
        ("Claude Code", folder(".claude")),
        ("Codex", folder(".agents")),
    ]
    .into_iter()
    .filter(|(name, _)| {
        args.claude == args.codex
            || (args.claude && *name == "Claude Code")
            || (args.codex && *name == "Codex")
    })
    .collect();
    for (name, destination) in agents {
        let outcome = install(&files, &destination, args.force)?;
        output::print(&format!("bigarrow: {name} skill {outcome}"));
    }
    Ok(())
}

fn install(files: &[(String, Vec<u8>)], destination: &Path, force: bool) -> Result<String> {
    let current = |file: &str| fs::read(destination.join(file)).ok();
    let changed: Vec<&(String, Vec<u8>)> = files
        .iter()
        .filter(|(file, data)| current(file).as_ref() != Some(data))
        .collect();
    if changed.is_empty() {
        return Ok(format!(
            "already installed at {}, nothing changed",
            destination.display()
        ));
    }
    let modified: Vec<&str> = changed
        .iter()
        .filter(|(file, _)| current(file).is_some())
        .map(|(file, _)| file.as_str())
        .collect();
    if !modified.is_empty() && !force {
        return Err(Error::bad_input(format!(
            "{} has changed files ({}); rerun with --force to replace them",
            destination.display(),
            modified.join(", ")
        )));
    }
    for (file, data) in &changed {
        let target = destination.join(file);
        let written = target
            .parent()
            .map_or(Ok(()), fs::create_dir_all)
            .and_then(|()| fs::write(&target, data));
        written.map_err(|error| {
            Error::bad_input(format!("cannot write {}: {error}", target.display()))
        })?;
    }
    let verb = if modified.is_empty() {
        "installed at"
    } else {
        "updated at"
    };
    Ok(format!("{verb} {}", destination.display()))
}

/// Every file of a skill folder, relative paths with `/`.
fn read_folder(folder: &Path) -> Result<Vec<(String, Vec<u8>)>> {
    let mut files = Vec::new();
    let mut pending = vec![folder.to_path_buf()];
    while let Some(directory) = pending.pop() {
        let entries = fs::read_dir(&directory).map_err(|_| {
            Error::bad_input(format!("cannot read the skill folder {}", folder.display()))
        })?;
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                pending.push(path);
            } else if let (Ok(relative), Ok(data)) = (path.strip_prefix(folder), fs::read(&path)) {
                files.push((relative.to_string_lossy().replace('\\', "/"), data));
            }
        }
    }
    if !files.iter().any(|(file, _)| file == "SKILL.md") {
        return Err(Error::bad_input(format!(
            "no SKILL.md in {}",
            folder.display()
        )));
    }
    files.sort();
    Ok(files)
}
