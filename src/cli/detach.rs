//! `--detach` and `start`: a child process shows the arrow the parent already planned, so an
//! agent's shell call returns at once and is never held open by the child.

use std::io::{BufRead, BufReader, Read, Write};
use std::sync::mpsc;
use std::time::Duration;

use clap::Parser;
use serde::{Deserialize, Serialize};

use super::config::{Mode, PointConfig};
use super::runner::{self, Planned};
use super::{Cli, Command, session};
use crate::model::error::{Error, Result};
use crate::model::placement::OtherArrow;
use crate::model::target::ResolvedTarget;
use crate::platform;
use crate::render::text::TextEngine;

const READY_TIMEOUT: Duration = Duration::from_secs(3);

/// What the child needs: the command line it was given, plus what the parent already decided, so
/// the child neither resolves the target again nor places the sign differently.
#[derive(Serialize, Deserialize)]
struct Job {
    args: Vec<String>,
    duration: f64,
    resolved: ResolvedTarget,
    others: Vec<(usize, OtherArrow)>,
}

/// Starts the child and returns its pid once it shows its arrow.
pub fn spawn(config: &PointConfig, planned: &Planned) -> Result<u32> {
    let job = Job {
        args: std::env::args().skip(1).collect(),
        duration: config.duration,
        resolved: planned.resolved.clone(),
        others: planned.others.clone(),
    };
    let mut child = platform::spawn_overlay().map_err(|error| {
        Error::bad_input(format!("could not start the detached arrow: {error}"))
    })?;
    let pid = child.id();
    if let Some(mut stdin) = child.stdin.take() {
        let _ = stdin.write_all(&serde_json::to_vec(&job).expect("jobs serialize"));
    }
    let stdout = child.stdout.take().expect("the child's stdout is piped");
    let (sender, receiver) = mpsc::channel();
    std::thread::spawn(move || {
        let mut line = String::new();
        let _ = BufReader::new(stdout).read_line(&mut line);
        let _ = sender.send(line);
    });
    match receiver.recv_timeout(READY_TIMEOUT) {
        Ok(line) if line.trim() == "ready" => Ok(pid),
        Ok(_) => {
            let code = child
                .wait()
                .ok()
                .and_then(|status| status.code())
                .filter(|code| *code != 0)
                .unwrap_or(3);
            Err(Error::new(
                code,
                format!("the detached arrow exited with code {code} before showing"),
            ))
        }
        Err(_) => {
            let _ = child.kill();
            Err(Error::unresolvable(format!(
                "the detached arrow (pid {pid}) did not show within {} s",
                READY_TIMEOUT.as_secs()
            )))
        }
    }
}

/// The child: reads its job from stdin and shows the arrow.
pub fn overlay() -> Result<()> {
    let mut input = String::new();
    let _ = std::io::stdin().read_to_string(&mut input);
    let job: Job = serde_json::from_str(&input)
        .map_err(|_| Error::bad_input("the detached arrow got no job on stdin"))?;
    let cli = Cli::try_parse_from(std::iter::once("bigarrow".to_owned()).chain(job.args))
        .map_err(|error| Error::bad_input(error.to_string()))?;
    let (args, mode) = match cli.command {
        Some(Command::Point(args)) => (args, Mode::Foreground),
        Some(Command::Start(args)) => (args, Mode::Background),
        _ => return Err(Error::bad_input("the detached arrow got no point command")),
    };
    let mut config = PointConfig::new(&args, mode)?;
    config.duration = job.duration;
    (config.raise, config.detach, config.dry_run, config.png) = (false, false, false, None);
    let screens = platform::screens()?;
    let mut engine = TextEngine::new();
    let planned = runner::plan(&config, job.resolved, &screens, &mut engine, &job.others)?;
    session::run(config, planned, engine, true)
}
