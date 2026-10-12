//! The `bigarrow` command line, compatible with bigarrow for macOS: same subcommands, flags,
//! defaults, messages and exit codes.

mod clear;
mod config;
mod demo;
mod detach;
mod doctor;
mod elements;
mod front;
mod install_skill;
mod options;
mod output;
mod runner;
mod session;
mod targeting;

use clap::{CommandFactory, Parser, Subcommand};

use crate::model::error::Error;
use config::{Mode, PointConfig};
use options::{
    ClearArgs, DemoArgs, DoctorArgs, ElementsArgs, FrontArgs, InstallSkillArgs, PointArgs,
};

const POINT_EXAMPLES: &str = "\
Pick exactly one target: --at, --rect, --mouse, --window or --element.

--app App[:window or tab title] (or --window) brings the target's app to the
front; the arrow hides while another app covers the target.
Every arrow ends by itself: time limit (point 8 s, start 300 s), 'stop',
or when the agent that drew it exits (CLAUDE_PID, BIGARROW_OWNER_PID).

  bigarrow point --at 760,500 --text \"Franz, click HERE\" --duration 10
  bigarrow start --rect 400,300,200,40 --app \"Google Chrome:Sign in\" \\
    --text \"Franz, sign in\"      then      bigarrow stop

More examples:
  bigarrow point --window \"Google Chrome\" --anchor title --text \"This window\"
  bigarrow point --element \"Reload\" --app \"Google Chrome\" --text \"Click reload\" --say
  bigarrow point --mouse --text \"You are here\" --duration 3
  bigarrow point --rect 400,300,200,80 --text \"Type your name\" --style box --color blue";

#[derive(Parser)]
#[command(
    name = "bigarrow",
    bin_name = "bigarrow",
    version,
    about = "Point at something on the screen with a big arrow and a sign. Click-through, gone by itself.",
    long_about = "Point at something on the screen with a big arrow and a sign. Click-through, gone by itself.\n\n\
        see-the-arrow for Windows and Linux; command-line compatible with bigarrow for macOS. \
        Coordinates are global top-left pixels (what UI Automation, AT-SPI on X11 and screenshots report). \
        Exit codes: 0 ok, 2 bad input, 3 target unresolvable, 4 permission missing.",
    args_override_self = true
)]
pub struct Cli {
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand)]
enum Command {
    /// Draw a big arrow with a sign that points at a target, for a time limit (default 8 s).
    #[command(after_long_help = POINT_EXAMPLES, args_override_self = true)]
    Point(PointArgs),
    /// Put an arrow up in the background until 'bigarrow stop' (same options as point).
    #[command(
        long_about = "Put an arrow up in the background until 'bigarrow stop' (same options as point).\n\n\
            Returns at once and prints the pid. Same as 'point --detach --duration 300'; --duration 0 = no limit.",
        args_override_self = true
    )]
    Start(PointArgs),
    /// Remove the newest live arrow (--all: every arrow, --pid: one, --session: one agent session's). Alias: stop.
    #[command(
        alias = "stop",
        long_about = "Remove the newest live arrow (--all: every arrow, --pid: one, --session: one agent session's). Alias: stop.\n\n\
            Run it once the human has acted. Arrows fade out and their processes exit 0.\n\
            --hook reads a Claude Code hook payload on stdin and silently clears that session's arrows;\n\
            as a UserPromptSubmit hook, every arrow of a session goes away when the human answers.",
        args_override_self = true
    )]
    Clear(ClearArgs),
    /// Bring an app (and optionally one of its windows) to the front before pointing.
    #[command(args_override_self = true)]
    Front(FrontArgs),
    /// List the visible, labelled UI elements of an app that --element can point at.
    #[command(args_override_self = true)]
    Elements(ElementsArgs),
    /// Show what this session can do (UI Automation / AT-SPI, elevation) and the displays.
    #[command(args_override_self = true)]
    Doctor(DoctorArgs),
    /// Show what bigarrow can do: an arrow, a ring at the mouse, a box, in sequence.
    #[command(args_override_self = true)]
    Demo(DemoArgs),
    /// Install the big-arrow skill for Claude Code and Codex (both by default).
    #[command(
        name = "install-skill",
        long_about = "Install the big-arrow skill for Claude Code and Codex (both by default).\n\n\
            One skill folder serves both agents: ~/.claude/skills/big-arrow (Claude Code) and \
            ~/.agents/skills/big-arrow (Codex). Idempotent; refuses to overwrite files you changed unless --force.",
        args_override_self = true
    )]
    InstallSkill(InstallSkillArgs),
    /// The detached arrow, started by `start` and `--detach`.
    #[command(name = "__overlay", hide = true)]
    Overlay,
}

/// Parses, runs, and maps every failure onto the documented exit codes.
pub fn main() -> i32 {
    let json = std::env::args().any(|arg| arg == "--json");
    let cli = match Cli::try_parse() {
        Ok(cli) => cli,
        Err(error) if !error.use_stderr() => {
            let _ = error.print();
            return 0;
        }
        Err(error) => output::fail(&Error::bad_input(clap_message(&error)), json),
    };
    let result = match &cli.command {
        None => {
            let _ = Cli::command().print_help();
            return 0;
        }
        Some(Command::Point(args)) => {
            PointConfig::new(args, Mode::Foreground).and_then(runner::run)
        }
        Some(Command::Start(args)) => {
            PointConfig::new(args, Mode::Background).and_then(runner::run)
        }
        Some(Command::Clear(args)) => clear::run(args),
        Some(Command::Front(args)) => front::run(args),
        Some(Command::Elements(args)) => elements::run(args),
        Some(Command::Doctor(args)) => doctor::run(args),
        Some(Command::Demo(args)) => demo::run(args),
        Some(Command::InstallSkill(args)) => install_skill::run(args),
        Some(Command::Overlay) => detach::overlay(),
    };
    match result {
        Ok(()) => 0,
        Err(error) => output::fail(&error, json),
    }
}

/// clap's error without its usage block, on one line.
fn clap_message(error: &clap::Error) -> String {
    let rendered = error.render().to_string();
    let lines: Vec<&str> = rendered
        .lines()
        .map(str::trim)
        .take_while(|line| !line.starts_with("Usage:"))
        .filter(|line| !line.is_empty())
        .collect();
    lines.join(" ").trim_start_matches("error: ").to_owned()
}
