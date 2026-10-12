//! `clear` (alias `stop`): ends live arrows. They fade out and their processes exit 0.

use std::io::Read;
use std::time::Duration;

use super::options::ClearArgs;
use super::output;
use crate::model::error::{Error, Result};
use crate::model::registry::{ArrowRecord, Registry};
use crate::model::result::ClearResult;
use crate::platform;

const WAIT_LIMIT: Duration = Duration::from_secs(1);

pub fn run(args: &ClearArgs) -> Result<()> {
    let registry = Registry::new(platform::registry_dir());
    if args.hook {
        let chosen: Vec<ArrowRecord> = hook_session()
            .map(|session| {
                registry
                    .live()
                    .into_iter()
                    .filter(|record| belongs_to(record, &session))
                    .collect()
            })
            .unwrap_or_default();
        clear(&registry, &chosen);
        return Ok(());
    }
    let chosen = choose(args, registry.live())?;
    let pids = clear(&registry, &chosen);
    if args.json {
        output::json(&ClearResult {
            cleared: pids,
            ok: true,
        });
    } else if pids.is_empty() {
        output::print("bigarrow: no live arrows");
    } else {
        let list: Vec<String> = pids.iter().map(u32::to_string).collect();
        output::print(&format!("bigarrow: cleared {}", list.join(", ")));
    }
    Ok(())
}

fn belongs_to(record: &ArrowRecord, session: &str) -> bool {
    record
        .owner
        .as_ref()
        .and_then(|owner| owner.session.as_deref())
        == Some(session)
}

fn choose(args: &ClearArgs, live: Vec<ArrowRecord>) -> Result<Vec<ArrowRecord>> {
    if let Some(session) = &args.session {
        return Ok(live
            .into_iter()
            .filter(|record| belongs_to(record, session))
            .collect());
    }
    if let Some(pid) = args.pid {
        let record = live.into_iter().find(|record| record.pid == pid);
        return record
            .map(|record| vec![record])
            .ok_or_else(|| Error::unresolvable(format!("no live arrow with pid {pid}")));
    }
    if args.all {
        return Ok(live);
    }
    Ok(live.into_iter().last().into_iter().collect())
}

/// Asks every chosen arrow to end, then waits until they are gone, at most one second.
fn clear(registry: &Registry, chosen: &[ArrowRecord]) -> Vec<u32> {
    let pids: Vec<u32> = chosen.iter().map(|record| record.pid).collect();
    for pid in &pids {
        platform::stop_arrow(*pid);
    }
    registry.wait_until_gone(&pids, WAIT_LIMIT);
    pids
}

/// The `session_id` of a Claude Code hook payload on stdin; `None` when there is none.
fn hook_session() -> Option<String> {
    let mut input = String::new();
    std::io::stdin().read_to_string(&mut input).ok()?;
    let payload: serde_json::Value = serde_json::from_str(&input).ok()?;
    payload
        .get("session_id")?
        .as_str()
        .filter(|id| !id.is_empty())
        .map(str::to_owned)
}
