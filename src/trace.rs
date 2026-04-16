//! Creates and propagates trace metadata across spawned child processes.

use std::env;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

pub const TRACE_ID_ENV: &str = "TRACE_ID";
pub const SPAN_ID_ENV: &str = "SPAN_ID";
pub const PARENT_SPAN_ID_ENV: &str = "PARENT_SPAN_ID";
pub const SPAWN_ID_ENV: &str = "SPAWN_ID";
pub const SESSION_ID_ENV: &str = "SESSION_ID";

static ID_COUNTER: AtomicU64 = AtomicU64::new(1);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TraceParent {
    pub trace_id: String,
    pub span_id: String,
    pub session_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TraceContext {
    pub trace_id: String,
    pub span_id: String,
    pub parent_span_id: Option<String>,
    pub spawn_id: String,
    pub session_id: String,
}

impl TraceParent {
    pub fn from_environment() -> Option<Self> {
        let trace_id = env::var(TRACE_ID_ENV).ok()?;
        let span_id = env::var(SPAN_ID_ENV).ok()?;
        let session_id = env::var(SESSION_ID_ENV).ok();

        Some(Self {
            trace_id,
            span_id,
            session_id,
        })
    }
}

impl TraceContext {
    pub fn new_root() -> Self {
        Self::child_of(None)
    }

    pub fn child_of(parent: Option<TraceParent>) -> Self {
        let trace_id = parent
            .as_ref()
            .map(|parent| parent.trace_id.clone())
            .unwrap_or_else(|| generate_id("trace"));
        let session_id = parent
            .as_ref()
            .and_then(|parent| parent.session_id.clone())
            .unwrap_or_else(|| generate_id("session"));

        Self {
            trace_id,
            span_id: generate_id("span"),
            parent_span_id: parent.map(|parent| parent.span_id),
            spawn_id: generate_id("spawn"),
            session_id,
        }
    }

    pub fn from_environment_or_root() -> Self {
        Self::child_of(TraceParent::from_environment())
    }

    pub fn as_child_process_env(&self) -> Vec<(String, String)> {
        vec![
            (TRACE_ID_ENV.to_string(), self.trace_id.clone()),
            (SPAN_ID_ENV.to_string(), self.span_id.clone()),
            (
                PARENT_SPAN_ID_ENV.to_string(),
                self.parent_span_id.clone().unwrap_or_default(),
            ),
            (SPAWN_ID_ENV.to_string(), self.spawn_id.clone()),
            (SESSION_ID_ENV.to_string(), self.session_id.clone()),
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
    use super::{
        PARENT_SPAN_ID_ENV, SESSION_ID_ENV, SPAN_ID_ENV, SPAWN_ID_ENV, TRACE_ID_ENV, TraceContext,
        TraceParent,
    };

    #[test]
    fn child_context_inherits_trace_parent_and_session() {
        let context = TraceContext::child_of(Some(TraceParent {
            trace_id: "trace-parent".into(),
            span_id: "span-parent".into(),
            session_id: Some("session-parent".into()),
        }));

        assert_eq!(context.trace_id, "trace-parent");
        assert_eq!(context.parent_span_id.as_deref(), Some("span-parent"));
        assert_eq!(context.session_id, "session-parent");
        assert!(context.span_id.starts_with("span-"));
        assert!(context.spawn_id.starts_with("spawn-"));
    }

    #[test]
    fn child_process_env_contains_required_fields() {
        let context = TraceContext {
            trace_id: "trace-1".into(),
            span_id: "span-1".into(),
            parent_span_id: Some("span-root".into()),
            spawn_id: "spawn-1".into(),
            session_id: "session-1".into(),
        };

        let env = context.as_child_process_env();

        assert!(env.contains(&(TRACE_ID_ENV.to_string(), "trace-1".into())));
        assert!(env.contains(&(SPAN_ID_ENV.to_string(), "span-1".into())));
        assert!(env.contains(&(PARENT_SPAN_ID_ENV.to_string(), "span-root".into())));
        assert!(env.contains(&(SPAWN_ID_ENV.to_string(), "spawn-1".into())));
        assert!(env.contains(&(SESSION_ID_ENV.to_string(), "session-1".into())));
    }

    #[test]
    fn root_context_has_empty_parent_env_value() {
        let context = TraceContext::new_root();
        let env = context.as_child_process_env();
        let parent_span = env
            .iter()
            .find(|(key, _)| key == PARENT_SPAN_ID_ENV)
            .map(|(_, value)| value.as_str());

        assert_eq!(parent_span, Some(""));
    }
}
