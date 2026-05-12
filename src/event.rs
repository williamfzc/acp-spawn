//! Defines the JSONL event model emitted by the spawn runtime.

use std::fmt;
use std::io::{self, Write};
use std::sync::mpsc::Sender;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

use crate::metadata::RunContext;

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
    pub run_id: String,
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
    pub run_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent_run_id: Option<String>,
}

impl EventContext {
    pub fn new(run_id: impl Into<String>, parent_run_id: Option<impl Into<String>>) -> Self {
        Self {
            run_id: run_id.into(),
            parent_run_id: parent_run_id.map(Into::into),
        }
    }

    pub fn from_run(run: &RunContext) -> Self {
        Self {
            run_id: run.run_id.clone(),
            parent_run_id: run.parent_run_id.clone(),
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuntimeEvent {
    SpawnStarted(EventEnvelope<SpawnStartedData>),
    SpawnCompleted(EventEnvelope<SpawnCompletedData>),
    SpawnFailed(EventEnvelope<SpawnFailedData>),
}

pub trait RuntimeEventSink: Send {
    fn handle(&mut self, event: &RuntimeEvent) -> Result<(), String>;
}

pub struct NoopEventSink;

impl RuntimeEventSink for NoopEventSink {
    fn handle(&mut self, _event: &RuntimeEvent) -> Result<(), String> {
        Ok(())
    }
}

pub struct ChannelEventSink {
    tx: Sender<RuntimeEvent>,
}

impl ChannelEventSink {
    pub fn new(tx: Sender<RuntimeEvent>) -> Self {
        Self { tx }
    }
}

impl RuntimeEventSink for ChannelEventSink {
    fn handle(&mut self, event: &RuntimeEvent) -> Result<(), String> {
        self.tx
            .send(event.clone())
            .map_err(|error| error.to_string())
    }
}

pub struct FanoutEventSink {
    sinks: Vec<Box<dyn RuntimeEventSink>>,
}

impl FanoutEventSink {
    pub fn new(sinks: Vec<Box<dyn RuntimeEventSink>>) -> Self {
        Self { sinks }
    }
}

impl RuntimeEventSink for FanoutEventSink {
    fn handle(&mut self, event: &RuntimeEvent) -> Result<(), String> {
        for sink in &mut self.sinks {
            sink.handle(event)?;
        }
        Ok(())
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

    pub fn write_raw_chunk(&mut self, chunk: &[u8]) -> Result<(), JsonlWriteError> {
        self.writer.write_all(chunk).map_err(JsonlWriteError::Io)?;
        self.writer.flush().map_err(JsonlWriteError::Io)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::io;
    use std::sync::mpsc;

    use serde_json::{Value, json};

    use super::{
        ChannelEventSink, EventContext, EventEnvelope, EventKind, JsonlWriteError, JsonlWriter,
        ResultStatus, RuntimeEvent, RuntimeEventSink, SpawnResult,
    };

    #[test]
    fn serializes_event_with_required_top_level_fields() {
        let event = EventEnvelope::new(
            EventContext::new("run-1", Some("run-root")),
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
                "run_id": "run-1",
                "parent_run_id": "run-root",
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
    fn writes_raw_child_chunk_without_wrapping() {
        let mut writer = JsonlWriter::new(Vec::new());
        writer
            .write_raw_chunk(br#"{"event":"tool_called","payload":{"tool":"ls"}}"#)
            .expect("raw chunk should write");

        let output = String::from_utf8(writer.into_inner()).expect("output should be UTF-8");
        assert_eq!(
            output,
            "{\"event\":\"tool_called\",\"payload\":{\"tool\":\"ls\"}}"
        );
    }

    #[test]
    fn writes_one_valid_json_object_per_line() {
        let started = EventEnvelope::new(
            EventContext::new("run-1", None::<String>),
            "2026-04-16T12:34:56Z",
            EventKind::SpawnStarted,
            json!({
                "spawn_id": "spawn-1",
            }),
        );
        let completed = EventEnvelope::new(
            EventContext::new("run-1", None::<String>),
            "2026-04-16T12:35:10Z",
            EventKind::SpawnCompleted,
            json!({
                "result": SpawnResult {
                    status: ResultStatus::Success,
                    summary: "finished".into(),
                    artifacts: vec![],
                    run_id: "run-1".into(),
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
            EventContext::new("run-1", None::<String>),
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

    #[test]
    fn channel_sink_forwards_runtime_event() {
        let (tx, rx) = mpsc::channel();
        let mut sink = ChannelEventSink::new(tx);
        let event = RuntimeEvent::SpawnStarted(EventEnvelope::new(
            EventContext::new("run-1", Some("run-root")),
            "2026-04-16T12:34:56Z",
            EventKind::SpawnStarted,
            super::SpawnStartedData {
                spawn_id: "spawn-1".into(),
                agent: "codex".into(),
                command: vec!["codex".into(), "run".into()],
                cwd: "/tmp".into(),
                timeout_ms: None,
                pid: None,
            },
        ));

        sink.handle(&event).expect("event should send");

        assert_eq!(rx.recv().expect("event should receive"), event);
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
