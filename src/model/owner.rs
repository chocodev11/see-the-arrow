use serde::{Deserialize, Serialize};

/// The agent session an arrow belongs to. A detached arrow inherits its parent's environment, so
/// it knows the agent's process (it ends when that process exits) and session id (a prompt hook
/// clears exactly that session's arrows). Claude Code exports both; other agents set the
/// `BIGARROW_*` variables.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct ArrowOwner {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pid: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session: Option<String>,
}

const PID_KEYS: [&str; 2] = ["BIGARROW_OWNER_PID", "CLAUDE_PID"];
const SESSION_KEYS: [&str; 2] = ["BIGARROW_SESSION", "CLAUDE_CODE_SESSION_ID"];

impl ArrowOwner {
    /// The first set key wins; a pid that is not a positive number is ignored.
    pub fn from_env(lookup: impl Fn(&str) -> Option<String>) -> Self {
        let first = |keys: [&str; 2]| {
            keys.iter()
                .filter_map(|key| lookup(key))
                .map(|value| value.trim().to_owned())
                .find(|value| !value.is_empty())
        };
        Self {
            pid: first(PID_KEYS)
                .and_then(|raw| raw.parse::<u32>().ok())
                .filter(|pid| *pid > 0),
            session: first(SESSION_KEYS),
        }
    }

    pub fn current() -> Self {
        Self::from_env(|key| std::env::var(key).ok())
    }

    pub fn is_empty(&self) -> bool {
        self.pid.is_none() && self.session.is_none()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_set_key_wins_and_bad_pids_are_ignored() {
        let env = |pairs: &'static [(&'static str, &'static str)]| {
            move |key: &str| {
                pairs
                    .iter()
                    .find(|(name, _)| *name == key)
                    .map(|(_, value)| (*value).to_owned())
            }
        };
        let owner = ArrowOwner::from_env(env(&[
            ("CLAUDE_PID", "42"),
            ("CLAUDE_CODE_SESSION_ID", " abc "),
        ]));
        assert_eq!(
            owner,
            ArrowOwner {
                pid: Some(42),
                session: Some("abc".into())
            }
        );
        let owner = ArrowOwner::from_env(env(&[("BIGARROW_OWNER_PID", "x"), ("CLAUDE_PID", "42")]));
        assert_eq!(owner.pid, None);
        assert!(ArrowOwner::from_env(env(&[("BIGARROW_OWNER_PID", "0")])).is_empty());
    }
}
