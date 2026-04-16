//! Defines the JSONL event model emitted by the spawn runtime.

use std::fmt;
use std::io::{self, Write};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

use crate::trace::TraceContext;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EventKind {
    SpawnStarted,
    SpawnCompleted,
    SpawnFailed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResultStatus {
    Success,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Artifact {
    pub kind: String,
    pub value: Value,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SpawnResult {
    pub status: ResultStatus,
    pub summary: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub artifacts: Vec<Artifact>,
    pub trace_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exit_code: Option<i32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SpawnStartedData {
    pub spawn_id: String,
    pub agent: String,
    pub command: Vec<String>,
    pub cwd: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timeout_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pid: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SpawnCompletedData {
    pub spawn_id: String,
    pub duration_ms: u128,
    pub exit_code: i32,
    pub result: SpawnResult,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SpawnFailedData {
    pub spawn_id: String,
    pub duration_ms: u128,
    pub reason: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exit_code: Option<i32>,
    pub result: SpawnResult,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EventContext {
    pub trace_id: String,
    pub span_id: String,
    pub parent_span_id: Option<String>,
    pub session_id: String,
}

impl EventContext {
    pub fn new(
        trace_id: impl Into<String>,
        span_id: impl Into<String>,
        parent_span_id: Option<impl Into<String>>,
        session_id: impl Into<String>,
    ) -> Self {
        Self {
            trace_id: trace_id.into(),
            span_id: span_id.into(),
            parent_span_id: parent_span_id.map(Into::into),
            session_id: session_id.into(),
        }
    }

    pub fn from_trace(trace: &TraceContext) -> Self {
        Self {
            trace_id: trace.trace_id.clone(),
            span_id: trace.span_id.clone(),
            parent_span_id: trace.parent_span_id.clone(),
            session_id: trace.session_id.clone(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EventEnvelope<D> {
    #[serde(flatten)]
    pub context: EventContext,
    pub timestamp: String,
    pub event: EventKind,
    pub data: D,
}

impl<D> EventEnvelope<D> {
    pub fn new(
        context: EventContext,
        timestamp: impl Into<String>,
        event: EventKind,
        data: D,
    ) -> Self {
        Self {
            context,
            timestamp: timestamp.into(),
            event,
            data,
        }
    }

    pub fn now(context: EventContext, event: EventKind, data: D) -> Result<Self, TimestampError> {
        Ok(Self::new(context, current_timestamp()?, event, data))
    }
}

pub fn current_timestamp() -> Result<String, TimestampError> {
    OffsetDateTime::now_utc()
        .format(&Rfc3339)
        .map_err(TimestampError)
}

#[derive(Debug)]
pub struct TimestampError(time::error::Format);

impl fmt::Display for TimestampError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "failed to format event timestamp: {}", self.0)
    }
}

impl std::error::Error for TimestampError {}

#[derive(Debug)]
pub enum JsonlWriteError {
    Serialize(serde_json::Error),
    Io(io::Error),
}

impl fmt::Display for JsonlWriteError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Serialize(error) => write!(f, "failed to serialize event as JSON: {error}"),
            Self::Io(error) => write!(f, "failed to write JSONL event: {error}"),
        }
    }
}

impl std::error::Error for JsonlWriteError {}

pub struct JsonlWriter<W> {
    writer: W,
}

impl<W> JsonlWriter<W> {
    pub fn new(writer: W) -> Self {
        Self { writer }
    }

    pub fn into_inner(self) -> W {
        self.writer
    }
}

impl<W: Write> JsonlWriter<W> {
    pub fn write_event<D: Serialize>(
        &mut self,
        event: &EventEnvelope<D>,
    ) -> Result<(), JsonlWriteError> {
        let mut encoded = serde_json::to_vec(event).map_err(JsonlWriteError::Serialize)?;
        encoded.push(b'\n');

        self.writer
            .write_all(&encoded)
            .map_err(JsonlWriteError::Io)?;
        self.writer.flush().map_err(JsonlWriteError::Io)?;
        Ok(())
    }

    pub fn write_raw_line(&mut self, line: &str) -> Result<(), JsonlWriteError> {
        self.writer
            .write_all(line.as_bytes())
            .map_err(JsonlWriteError::Io)?;
        self.writer.write_all(b"\n").map_err(JsonlWriteError::Io)?;
        self.writer.flush().map_err(JsonlWriteError::Io)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::io;

    use serde_json::{Value, json};

    use super::{
        EventContext, EventEnvelope, EventKind, JsonlWriteError, JsonlWriter, ResultStatus,
        SpawnResult,
    };

    #[test]
    fn serializes_event_with_required_top_level_fields() {
        let event = EventEnvelope::new(
            EventContext::new("trace-1", "span-1", Some("parent-1"), "session-1"),
            "2026-04-16T12:34:56Z",
            EventKind::SpawnStarted,
            json!({
                "spawn_id": "spawn-1",
                "agent": "codex",
                "command": ["codex", "run"],
            }),
        );

        let value = serde_json::to_value(&event).expect("event should serialize");

        assert_eq!(
            value,
            json!({
                "trace_id": "trace-1",
                "span_id": "span-1",
                "parent_span_id": "parent-1",
                "session_id": "session-1",
                "timestamp": "2026-04-16T12:34:56Z",
                "event": "spawn_started",
                "data": {
                    "spawn_id": "spawn-1",
                    "agent": "codex",
                    "command": ["codex", "run"],
                }
            })
        );
    }

    #[test]
    fn writes_raw_child_line_without_wrapping() {
        let mut writer = JsonlWriter::new(Vec::new());
        writer
            .write_raw_line(r#"{"event":"tool_called","payload":{"tool":"ls"}}"#)
            .expect("raw line should write");

        let output = String::from_utf8(writer.into_inner()).expect("output should be UTF-8");
        assert_eq!(
            output,
            "{\"event\":\"tool_called\",\"payload\":{\"tool\":\"ls\"}}\n"
        );
    }

    #[test]
    fn writes_one_valid_json_object_per_line() {
        let started = EventEnvelope::new(
            EventContext::new("trace-1", "span-1", None::<String>, "session-1"),
            "2026-04-16T12:34:56Z",
            EventKind::SpawnStarted,
            json!({
                "spawn_id": "spawn-1",
            }),
        );
        let completed = EventEnvelope::new(
            EventContext::new("trace-1", "span-1", None::<String>, "session-1"),
            "2026-04-16T12:35:10Z",
            EventKind::SpawnCompleted,
            json!({
                "result": SpawnResult {
                    status: ResultStatus::Success,
                    summary: "finished".into(),
                    artifacts: vec![],
                    trace_id: "trace-1".into(),
                    error: None,
                    exit_code: Some(0),
                }
            }),
        );

        let mut writer = JsonlWriter::new(Vec::new());
        writer
            .write_event(&started)
            .expect("first event should write");
        writer
            .write_event(&completed)
            .expect("second event should write");

        let output = String::from_utf8(writer.into_inner()).expect("output should be UTF-8");
        let lines: Vec<&str> = output.lines().collect();

        assert_eq!(lines.len(), 2);
        assert!(output.ends_with('\n'));

        let first: Value = serde_json::from_str(lines[0]).expect("first line should be JSON");
        let second: Value = serde_json::from_str(lines[1]).expect("second line should be JSON");

        assert_eq!(first["event"], "spawn_started");
        assert_eq!(second["event"], "spawn_completed");
    }

    #[test]
    fn surfaces_writer_errors() {
        let event = EventEnvelope::new(
            EventContext::new("trace-1", "span-1", None::<String>, "session-1"),
            "2026-04-16T12:34:56Z",
            EventKind::SpawnFailed,
            json!({
                "reason": "spawn command not found",
            }),
        );
        let mut writer = JsonlWriter::new(FailingWriter);

        let error = writer
            .write_event(&event)
            .expect_err("writer should surface io failure");

        assert!(matches!(error, JsonlWriteError::Io(_)));
    }

    struct FailingWriter;

    impl io::Write for FailingWriter {
        fn write(&mut self, _buf: &[u8]) -> io::Result<usize> {
            Err(io::Error::new(
                io::ErrorKind::BrokenPipe,
                "simulated write failure",
            ))
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }
}
