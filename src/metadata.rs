//! Creates and propagates lightweight run metadata across spawned child processes.

use std::env;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

pub const RUN_ID_ENV: &str = "RUN_ID";
pub const PARENT_RUN_ID_ENV: &str = "PARENT_RUN_ID";
pub const SPAWN_ID_ENV: &str = "SPAWN_ID";

static ID_COUNTER: AtomicU64 = AtomicU64::new(1);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunParent {
    pub run_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunContext {
    pub run_id: String,
    pub parent_run_id: Option<String>,
    pub spawn_id: String,
}

impl RunParent {
    pub fn from_environment() -> Option<Self> {
        let run_id = env::var(RUN_ID_ENV).ok()?;
        Some(Self { run_id })
    }
}

impl RunContext {
    pub fn new_root() -> Self {
        Self::child_of(None)
    }

    pub fn child_of(parent: Option<RunParent>) -> Self {
        Self {
            run_id: generate_id("run"),
            parent_run_id: parent.map(|parent| parent.run_id),
            spawn_id: generate_id("spawn"),
        }
    }

    pub fn from_environment_or_root() -> Self {
        Self::child_of(RunParent::from_environment())
    }

    pub fn as_child_process_env(&self) -> Vec<(String, String)> {
        vec![
            (RUN_ID_ENV.to_string(), self.run_id.clone()),
            (
                PARENT_RUN_ID_ENV.to_string(),
                self.parent_run_id.clone().unwrap_or_default(),
            ),
            (SPAWN_ID_ENV.to_string(), self.spawn_id.clone()),
        ]
    }
}

fn generate_id(kind: &str) -> String {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let pid = std::process::id();
    let counter = ID_COUNTER.fetch_add(1, Ordering::Relaxed);

    format!("{kind}-{pid:08x}-{now:032x}-{counter:08x}")
}

#[cfg(test)]
mod tests {
    use super::{PARENT_RUN_ID_ENV, RUN_ID_ENV, RunContext, RunParent, SPAWN_ID_ENV};

    #[test]
    fn child_context_records_parent_run_id() {
        let context = RunContext::child_of(Some(RunParent {
            run_id: "run-parent".into(),
        }));

        assert_eq!(context.parent_run_id.as_deref(), Some("run-parent"));
        assert!(context.run_id.starts_with("run-"));
        assert!(context.spawn_id.starts_with("spawn-"));
    }

    #[test]
    fn child_process_env_contains_required_fields() {
        let context = RunContext {
            run_id: "run-1".into(),
            parent_run_id: Some("run-root".into()),
            spawn_id: "spawn-1".into(),
        };

        let env = context.as_child_process_env();

        assert!(env.contains(&(RUN_ID_ENV.to_string(), "run-1".into())));
        assert!(env.contains(&(PARENT_RUN_ID_ENV.to_string(), "run-root".into())));
        assert!(env.contains(&(SPAWN_ID_ENV.to_string(), "spawn-1".into())));
    }

    #[test]
    fn root_context_has_empty_parent_run_id_env_value() {
        let context = RunContext::new_root();
        let env = context.as_child_process_env();
        let parent_run_id = env
            .iter()
            .find(|(key, _)| key == PARENT_RUN_ID_ENV)
            .map(|(_, value)| value.as_str());

        assert_eq!(parent_run_id, Some(""));
    }
}
