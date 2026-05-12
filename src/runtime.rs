//! Runs child agents and emits structured JSONL lifecycle events.

use std::cell::RefCell;
use std::ffi::OsString;
use std::fmt;
use std::fs::File;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::cancel::{CancelError, CancellationHandle};
use crate::cli::RunArgs;
use crate::config::{self, ConfigError};
use crate::event::{
    EventContext, EventEnvelope, EventKind, JsonlWriteError, JsonlWriter, NoopEventSink,
    ResultStatus, RuntimeEvent, RuntimeEventSink, SpawnCompletedData, SpawnFailedData, SpawnResult,
    SpawnStartedData, TimestampError,
};
use crate::metadata::RunContext;
use crate::process::{self, ProcessError, ProcessSpec, ProcessTermination};

pub const ACP_SPAWN_GOAL_ENV: &str = "ACP_SPAWN_GOAL";
pub const DEFAULT_TIMEOUT_MS: u64 = 300_000;
const TERMINATION_GRACE_PERIOD_MS: u64 = 500;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunRequest {
    pub agent: String,
    pub agent_args: Vec<String>,
    pub goal: String,
    pub cwd: PathBuf,
    pub timeout: Option<Duration>,
    pub input_file: Option<PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunOutcome {
    pub run: RunContext,
    pub result: SpawnResult,
}

#[derive(Debug)]
pub enum RuntimeError {
    MissingCurrentDirectory,
    CwdDoesNotExist(PathBuf),
    CwdIsNotDirectory(PathBuf),
    MissingRunField(&'static str),
    InputOpenFailed {
        path: PathBuf,
        reason: String,
    },
    CancelSetup(CancelError),
    Config(ConfigError),
    Process(ProcessError),
    EventTimestamp(TimestampError),
    EventWrite(JsonlWriteError),
    EventSink(String),
    StderrWrite(io::Error),
    ChildExitedNonZero {
        agent: String,
        exit_code: i32,
        run_id: String,
        spawn_id: String,
    },
    ChildTimedOut {
        agent: String,
        timeout_ms: u64,
        run_id: String,
        spawn_id: String,
    },
    ChildCancelled {
        agent: String,
        reason: String,
        run_id: String,
        spawn_id: String,
    },
    ChildTerminated {
        agent: String,
        run_id: String,
        spawn_id: String,
    },
}

impl fmt::Display for RuntimeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingCurrentDirectory => {
                write!(f, "failed to determine current working directory")
            }
            Self::CwdDoesNotExist(path) => {
                write!(f, "working directory does not exist: {}", path.display())
            }
            Self::CwdIsNotDirectory(path) => {
                write!(
                    f,
                    "working directory is not a directory: {}",
                    path.display()
                )
            }
            Self::MissingRunField(field) => {
                write!(f, "missing required run setting: {field}")
            }
            Self::InputOpenFailed { path, reason } => {
                write!(
                    f,
                    "failed to open input file '{}': {reason}",
                    path.display()
                )
            }
            Self::CancelSetup(error) => write!(f, "{error}"),
            Self::Config(error) => write!(f, "{error}"),
            Self::Process(error) => write!(f, "{error}"),
            Self::EventTimestamp(error) => write!(f, "{error}"),
            Self::EventWrite(error) => write!(f, "{error}"),
            Self::EventSink(error) => write!(f, "failed to write side-channel event: {error}"),
            Self::StderrWrite(error) => write!(f, "failed to write stderr: {error}"),
            Self::ChildExitedNonZero {
                agent,
                exit_code,
                run_id,
                spawn_id,
            } => write!(
                f,
                "child agent '{agent}' exited with code {exit_code} (run_id={run_id}, spawn_id={spawn_id})"
            ),
            Self::ChildTimedOut {
                agent,
                timeout_ms,
                run_id,
                spawn_id,
            } => write!(
                f,
                "child agent '{agent}' timed out after {timeout_ms}ms (run_id={run_id}, spawn_id={spawn_id})"
            ),
            Self::ChildCancelled {
                agent,
                reason,
                run_id,
                spawn_id,
            } => write!(
                f,
                "child agent '{agent}' was cancelled: {reason} (run_id={run_id}, spawn_id={spawn_id})"
            ),
            Self::ChildTerminated {
                agent,
                run_id,
                spawn_id,
            } => write!(
                f,
                "child agent '{agent}' terminated without an exit code (run_id={run_id}, spawn_id={spawn_id})"
            ),
        }
    }
}

impl std::error::Error for RuntimeError {}

impl From<ProcessError> for RuntimeError {
    fn from(value: ProcessError) -> Self {
        Self::Process(value)
    }
}

impl From<ConfigError> for RuntimeError {
    fn from(value: ConfigError) -> Self {
        Self::Config(value)
    }
}

impl From<TimestampError> for RuntimeError {
    fn from(value: TimestampError) -> Self {
        Self::EventTimestamp(value)
    }
}

impl From<JsonlWriteError> for RuntimeError {
    fn from(value: JsonlWriteError) -> Self {
        Self::EventWrite(value)
    }
}

impl TryFrom<RunArgs> for RunRequest {
    type Error = RuntimeError;

    fn try_from(args: RunArgs) -> Result<Self, Self::Error> {
        let loaded = match args.config.as_ref() {
            Some(path) => Some(config::load_run_config(path, args.profile.as_deref())?),
            None => None,
        };

        Ok(Self {
            agent: args
                .agent
                .or_else(|| loaded.as_ref().map(|config| config.agent.clone()))
                .ok_or(RuntimeError::MissingRunField("agent"))?,
            agent_args: if args.agent_args.is_empty() {
                loaded
                    .as_ref()
                    .map(|config| config.agent_args.clone())
                    .unwrap_or_default()
            } else {
                args.agent_args
            },
            goal: args
                .goal
                .or_else(|| loaded.as_ref().map(|config| config.goal.clone()))
                .ok_or(RuntimeError::MissingRunField("goal"))?,
            cwd: args
                .cwd
                .or_else(|| loaded.as_ref().map(|config| config.cwd.clone()))
                .ok_or(RuntimeError::MissingRunField("cwd"))?,
            timeout: args
                .timeout_ms
                .or_else(|| loaded.as_ref().and_then(|config| config.timeout_ms))
                .map(Duration::from_millis),
            input_file: args.input_file,
        })
    }
}

pub fn run(request: RunRequest) -> Result<RunOutcome, RuntimeError> {
    let cancellation =
        CancellationHandle::install_signal_handlers().map_err(RuntimeError::CancelSetup)?;
    let stdout = io::stdout();
    let stderr = io::stderr();

    run_with_io(request, &cancellation, stdout.lock(), stderr.lock())
}

pub fn run_with_io<W: Write, E: Write>(
    request: RunRequest,
    cancellation: &CancellationHandle,
    stdout: W,
    stderr: E,
) -> Result<RunOutcome, RuntimeError> {
    run_with_event_sink(request, cancellation, stdout, stderr, NoopEventSink)
}

pub fn run_with_event_sink<W: Write, E: Write, S: RuntimeEventSink>(
    request: RunRequest,
    cancellation: &CancellationHandle,
    stdout: W,
    stderr: E,
    event_sink: S,
) -> Result<RunOutcome, RuntimeError> {
    let run = RunContext::from_environment_or_root();
    let timeout = request
        .timeout
        .unwrap_or_else(|| Duration::from_millis(DEFAULT_TIMEOUT_MS));
    let timeout_ms = duration_to_millis(timeout);
    let mut emitter = RuntimeEmitter::new(run.clone(), stdout, stderr, event_sink);

    let cwd = match resolve_cwd(&request.cwd) {
        Ok(cwd) => cwd,
        Err(error) => {
            emitter.emit_failed(
                0,
                failure_result(&run, "failed to determine current working directory", None),
                "failed to determine current working directory",
                None,
            )?;
            return Err(error);
        }
    };

    if let Err(error) = validate_cwd(&cwd) {
        emitter.emit_failed(
            0,
            failure_result(&run, &error.to_string(), None),
            &error.to_string(),
            None,
        )?;
        return Err(error);
    }

    let spec = ProcessSpec {
        program: OsString::from(&request.agent),
        args: request.agent_args.iter().map(OsString::from).collect(),
        cwd: cwd.clone(),
        env: child_process_env(&request.goal, &run),
        timeout: Some(timeout),
        termination_grace_period: Duration::from_millis(TERMINATION_GRACE_PERIOD_MS),
    };

    let input = match request.input_file.as_ref() {
        Some(input_path) => match File::open(input_path) {
            Ok(input) => Some(input),
            Err(error) => {
                let error = RuntimeError::InputOpenFailed {
                    path: input_path.clone(),
                    reason: error.to_string(),
                };
                emitter.emit_failed(
                    0,
                    failure_result(&run, &error.to_string(), None),
                    &error.to_string(),
                    None,
                )?;
                return Err(error);
            }
        },
        None => None,
    };

    let mut running = match process::spawn(&spec) {
        Ok(running) => running,
        Err(error) => {
            emitter.emit_failed(
                0,
                failure_result(&run, &error.to_string(), None),
                &error.to_string(),
                None,
            )?;
            return Err(RuntimeError::Process(error));
        }
    };

    if let Some(input) = input {
        running.start_stdin_forwarder(input);
    } else {
        running.close_stdin();
    }

    emitter.emit_started(
        &request.agent,
        &request.agent_args,
        &cwd,
        timeout_ms,
        Some(running.pid()),
    )?;

    let emitter = RefCell::new(emitter);
    let output = running.wait_with_streaming(
        cancellation,
        &mut |chunk| emitter.borrow_mut().passthrough_stdout_chunk(chunk),
        &mut |chunk| emitter.borrow_mut().write_stderr_chunk(chunk),
    )?;
    let mut emitter = emitter.into_inner();
    let duration_ms = duration_to_millis_u128(output.duration);

    match output.termination {
        ProcessTermination::Exited if output.exit_code == Some(0) => {
            let result = SpawnResult {
                status: ResultStatus::Success,
                summary: format!("agent '{}' completed successfully", request.agent),
                artifacts: vec![],
                run_id: run.run_id.clone(),
                error: None,
                exit_code: output.exit_code,
            };
            emitter.emit_completed(duration_ms, output.exit_code.unwrap_or(0), result.clone())?;

            Ok(RunOutcome { run, result })
        }
        ProcessTermination::TimedOut => {
            let reason = format!("child process timed out after {timeout_ms}ms");
            let result = failure_result(&run, &reason, output.exit_code);
            emitter.emit_failed(duration_ms, result, &reason, output.exit_code)?;
            Err(RuntimeError::ChildTimedOut {
                agent: request.agent,
                timeout_ms,
                run_id: run.run_id,
                spawn_id: run.spawn_id,
            })
        }
        ProcessTermination::Cancelled { reason } => {
            let result = failure_result(&run, &reason, output.exit_code);
            emitter.emit_failed(duration_ms, result, &reason, output.exit_code)?;
            Err(RuntimeError::ChildCancelled {
                agent: request.agent,
                reason,
                run_id: run.run_id,
                spawn_id: run.spawn_id,
            })
        }
        ProcessTermination::Exited => {
            let error = match output.exit_code {
                Some(exit_code) => RuntimeError::ChildExitedNonZero {
                    agent: request.agent.clone(),
                    exit_code,
                    run_id: run.run_id.clone(),
                    spawn_id: run.spawn_id.clone(),
                },
                None => RuntimeError::ChildTerminated {
                    agent: request.agent.clone(),
                    run_id: run.run_id.clone(),
                    spawn_id: run.spawn_id.clone(),
                },
            };
            let result = failure_result(&run, &error.to_string(), output.exit_code);
            emitter.emit_failed(duration_ms, result, &error.to_string(), output.exit_code)?;
            Err(error)
        }
    }
}

fn failure_result(run: &RunContext, summary: &str, exit_code: Option<i32>) -> SpawnResult {
    SpawnResult {
        status: ResultStatus::Failed,
        summary: summary.to_string(),
        artifacts: vec![],
        run_id: run.run_id.clone(),
        error: Some(summary.to_string()),
        exit_code,
    }
}

fn child_process_env(goal: &str, run: &RunContext) -> Vec<(String, String)> {
    let mut env = run.as_child_process_env();
    env.push((ACP_SPAWN_GOAL_ENV.to_string(), goal.to_string()));
    env
}

fn resolve_cwd(cwd: &Path) -> Result<PathBuf, RuntimeError> {
    if cwd.is_absolute() {
        return Ok(cwd.to_path_buf());
    }

    let current_dir = std::env::current_dir().map_err(|_| RuntimeError::MissingCurrentDirectory)?;
    Ok(current_dir.join(cwd))
}

fn validate_cwd(cwd: &Path) -> Result<(), RuntimeError> {
    if !cwd.exists() {
        return Err(RuntimeError::CwdDoesNotExist(cwd.to_path_buf()));
    }

    if !cwd.is_dir() {
        return Err(RuntimeError::CwdIsNotDirectory(cwd.to_path_buf()));
    }

    Ok(())
}

fn duration_to_millis(duration: Duration) -> u64 {
    duration.as_millis().try_into().unwrap_or(u64::MAX)
}

fn duration_to_millis_u128(duration: Duration) -> u128 {
    duration.as_millis()
}

struct RuntimeEmitter<W, E, S> {
    context: EventContext,
    run: RunContext,
    stdout: JsonlWriter<W>,
    stderr: E,
    event_sink: S,
}

impl<W: Write, E: Write, S: RuntimeEventSink> RuntimeEmitter<W, E, S> {
    fn new(run: RunContext, stdout: W, stderr: E, event_sink: S) -> Self {
        Self {
            context: EventContext::from_run(&run),
            run,
            stdout: JsonlWriter::new(stdout),
            stderr,
            event_sink,
        }
    }

    fn emit_started(
        &mut self,
        agent: &str,
        agent_args: &[String],
        cwd: &Path,
        timeout_ms: u64,
        pid: Option<u32>,
    ) -> Result<(), RuntimeError> {
        let mut command = vec![agent.to_string()];
        command.extend(agent_args.iter().cloned());
        self.write_lifecycle_event(RuntimeEvent::SpawnStarted(EventEnvelope::now(
            self.context.clone(),
            EventKind::SpawnStarted,
            SpawnStartedData {
                spawn_id: self.run.spawn_id.clone(),
                agent: agent.to_string(),
                command,
                cwd: cwd.display().to_string(),
                timeout_ms: Some(timeout_ms),
                pid,
            },
        )?))
    }

    fn passthrough_stdout_chunk(&mut self, chunk: &[u8]) -> Result<(), String> {
        self.stdout
            .write_raw_chunk(chunk)
            .map_err(|error| error.to_string())
    }

    fn emit_completed(
        &mut self,
        duration_ms: u128,
        exit_code: i32,
        result: SpawnResult,
    ) -> Result<(), RuntimeError> {
        self.write_lifecycle_event(RuntimeEvent::SpawnCompleted(EventEnvelope::now(
            self.context.clone(),
            EventKind::SpawnCompleted,
            SpawnCompletedData {
                spawn_id: self.run.spawn_id.clone(),
                duration_ms,
                exit_code,
                result,
            },
        )?))
    }

    fn emit_failed(
        &mut self,
        duration_ms: u128,
        result: SpawnResult,
        reason: &str,
        exit_code: Option<i32>,
    ) -> Result<(), RuntimeError> {
        self.write_lifecycle_event(RuntimeEvent::SpawnFailed(EventEnvelope::now(
            self.context.clone(),
            EventKind::SpawnFailed,
            SpawnFailedData {
                spawn_id: self.run.spawn_id.clone(),
                duration_ms,
                reason: reason.to_string(),
                exit_code,
                result,
            },
        )?))
    }

    fn write_stderr_chunk(&mut self, chunk: &[u8]) -> Result<(), String> {
        self.stderr
            .write_all(chunk)
            .and_then(|_| self.stderr.flush())
            .map_err(|error| error.to_string())
    }

    fn write_lifecycle_event(&mut self, event: RuntimeEvent) -> Result<(), RuntimeError> {
        match &event {
            RuntimeEvent::SpawnStarted(envelope) => self.stdout.write_event(envelope)?,
            RuntimeEvent::SpawnCompleted(envelope) => self.stdout.write_event(envelope)?,
            RuntimeEvent::SpawnFailed(envelope) => self.stdout.write_event(envelope)?,
        }
        self.event_sink
            .handle(&event)
            .map_err(RuntimeError::EventSink)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::env;
    use std::fs;
    use std::io::{self, Write};
    use std::path::PathBuf;
    use std::sync::mpsc;
    use std::sync::{Arc, Mutex};
    use std::thread;
    use std::time::{Duration, SystemTime, UNIX_EPOCH};

    use serde_json::Value;

    use crate::cancel::CancellationHandle;
    use crate::event::{ChannelEventSink, RuntimeEvent};
    use crate::metadata::{PARENT_RUN_ID_ENV, RUN_ID_ENV};

    use super::{
        ACP_SPAWN_GOAL_ENV, RunContext, RunRequest, RuntimeError, run_with_event_sink, run_with_io,
    };

    #[cfg(unix)]
    use std::os::unix::fs::PermissionsExt;

    #[cfg(unix)]
    #[test]
    fn run_emits_lifecycle_events_and_passthrough_child_stdout() {
        let cwd = create_temp_dir("runtime-success");
        let script = create_script(
            "capture-runtime.sh",
            r#"#!/bin/sh
printf '{"event":"tool_called","payload":{"tool":"ls"}}\n'
printf 'stderr-line\n' >&2
"#,
        );

        let stdout = SharedBuffer::default();
        let stderr = SharedBuffer::default();
        let outcome = run_with_io(
            RunRequest {
                agent: script.to_string_lossy().into_owned(),
                agent_args: vec![],
                goal: "implement parser".into(),
                cwd,
                timeout: Some(Duration::from_millis(5_000)),
                input_file: None,
            },
            &CancellationHandle::new(),
            stdout.writer(),
            stderr.writer(),
        )
        .expect("run should succeed");

        let events = parse_json_lines(&stdout.contents());
        assert_eq!(events[0]["event"], "spawn_started");
        assert_eq!(
            events[0]["data"]["command"],
            serde_json::json!([script.to_string_lossy().to_string()])
        );
        assert_eq!(events[1]["event"], "tool_called");
        assert_eq!(events[1]["payload"]["tool"], "ls");
        assert_eq!(events[2]["event"], "spawn_completed");
        assert_eq!(events[2]["data"]["result"]["status"], "success");
        assert_eq!(outcome.result.status, crate::event::ResultStatus::Success);
        assert_eq!(stderr.contents(), "stderr-line\n");
    }

    #[cfg(unix)]
    #[test]
    fn run_side_channel_receives_lifecycle_events_only() {
        let cwd = create_temp_dir("runtime-side-channel");
        let (tx, rx) = mpsc::channel();
        let stdout = SharedBuffer::default();
        let stderr = SharedBuffer::default();

        run_with_event_sink(
            RunRequest {
                agent: "/bin/sh".into(),
                agent_args: vec![
                    "-c".into(),
                    "printf '{\"event\":\"child_stdout\"}\\n'".into(),
                ],
                goal: "implement parser".into(),
                cwd,
                timeout: Some(Duration::from_millis(10_000)),
                input_file: None,
            },
            &CancellationHandle::new(),
            stdout.writer(),
            stderr.writer(),
            ChannelEventSink::new(tx),
        )
        .expect("run should succeed");

        let received: Vec<RuntimeEvent> = rx.try_iter().collect();

        assert_eq!(received.len(), 2);
        assert!(matches!(received[0], RuntimeEvent::SpawnStarted(_)));
        assert!(matches!(received[1], RuntimeEvent::SpawnCompleted(_)));

        let stdout_events = parse_json_lines(&stdout.contents());
        assert_eq!(stdout_events.len(), 3);
        assert_eq!(stdout_events[1]["event"], "child_stdout");
        assert_eq!(stderr.contents(), "");
    }

    #[cfg(unix)]
    #[test]
    fn run_passes_agent_args_to_child_process() {
        let cwd = create_temp_dir("runtime-args");
        let stdout = SharedBuffer::default();
        let stderr = SharedBuffer::default();

        run_with_io(
            RunRequest {
                agent: "/bin/sh".into(),
                agent_args: vec![
                    "-c".into(),
                    "printf '{\"event\":\"argv_check\",\"args\":[\"%s\",\"%s\"]}\\n' \"$0\" \"$1\""
                        .into(),
                    "first".into(),
                    "second".into(),
                ],
                goal: "verify args".into(),
                cwd,
                timeout: Some(Duration::from_millis(5_000)),
                input_file: None,
            },
            &CancellationHandle::new(),
            stdout.writer(),
            stderr.writer(),
        )
        .expect("run should succeed");

        let events = parse_json_lines(&stdout.contents());
        assert_eq!(
            events[0]["data"]["command"],
            serde_json::json!([
                "/bin/sh",
                "-c",
                "printf '{\"event\":\"argv_check\",\"args\":[\"%s\",\"%s\"]}\\n' \"$0\" \"$1\"",
                "first",
                "second"
            ])
        );
        assert_eq!(events[1]["event"], "argv_check");
        assert_eq!(events[1]["args"], serde_json::json!(["first", "second"]));
        assert_eq!(stderr.contents(), "");
    }

    #[cfg(unix)]
    #[test]
    fn run_closes_child_stdin_when_no_input_is_forwarded() {
        let cwd = create_temp_dir("runtime-stdin-closed");
        let stdout = SharedBuffer::default();
        let stderr = SharedBuffer::default();

        run_with_io(
            RunRequest {
                agent: "/bin/sh".into(),
                agent_args: vec![
                    "-c".into(),
                    "cat >/dev/null; printf '{\"event\":\"stdin_closed\"}\\n'".into(),
                ],
                goal: "stdin closed".into(),
                cwd,
                timeout: Some(Duration::from_millis(200)),
                input_file: None,
            },
            &CancellationHandle::new(),
            stdout.writer(),
            stderr.writer(),
        )
        .expect("run should finish after child receives EOF");

        let events = parse_json_lines(&stdout.contents());
        assert_eq!(events[1]["event"], "stdin_closed");
        assert_eq!(stderr.contents(), "");
    }

    #[cfg(unix)]
    #[test]
    fn run_records_parent_run_id_from_environment() {
        let cwd = create_temp_dir("runtime-inherit");
        let script = create_script(
            "inherit-trace.sh",
            r#"#!/bin/sh
printf '{"event":"run_check","run":"%s","parent":"%s"}\n' "$RUN_ID" "$PARENT_RUN_ID"
"#,
        );

        unsafe {
            env::set_var(RUN_ID_ENV, "run-parent");
        }

        let stdout = SharedBuffer::default();
        let stderr = SharedBuffer::default();
        let outcome = run_with_io(
            RunRequest {
                agent: script.to_string_lossy().into_owned(),
                agent_args: vec![],
                goal: "inherit".into(),
                cwd,
                timeout: Some(Duration::from_millis(5_000)),
                input_file: None,
            },
            &CancellationHandle::new(),
            stdout.writer(),
            stderr.writer(),
        )
        .expect("run should succeed");

        unsafe {
            env::remove_var(RUN_ID_ENV);
        }

        let events = parse_json_lines(&stdout.contents());
        assert_eq!(outcome.run.parent_run_id.as_deref(), Some("run-parent"));
        assert_eq!(events[1]["parent"], "run-parent");
    }

    #[cfg(unix)]
    #[test]
    fn run_emits_failed_event_for_non_zero_exit() {
        let cwd = create_temp_dir("runtime-failure");
        let script = create_script(
            "exit-9.sh",
            r#"#!/bin/sh
exit 9
"#,
        );

        let stdout = SharedBuffer::default();
        let error = run_with_io(
            RunRequest {
                agent: script.to_string_lossy().into_owned(),
                agent_args: vec![],
                goal: "fail".into(),
                cwd,
                timeout: Some(Duration::from_millis(5_000)),
                input_file: None,
            },
            &CancellationHandle::new(),
            stdout.writer(),
            SharedBuffer::default().writer(),
        )
        .expect_err("run should fail");

        let events = parse_json_lines(&stdout.contents());
        assert_eq!(
            events.last().expect("failed event")["event"],
            "spawn_failed"
        );
        assert_eq!(
            events.last().expect("failed event")["data"]["result"]["status"],
            "failed"
        );

        match error {
            RuntimeError::ChildExitedNonZero { exit_code, .. } => assert_eq!(exit_code, 9),
            other => panic!("unexpected error: {other:?}"),
        }
    }

    #[cfg(unix)]
    #[test]
    fn run_emits_failed_event_for_timeout() {
        let cwd = create_temp_dir("runtime-timeout");
        let script = create_script(
            "sleep.sh",
            r#"#!/bin/sh
sleep 5
"#,
        );

        let stdout = SharedBuffer::default();
        let error = run_with_io(
            RunRequest {
                agent: script.to_string_lossy().into_owned(),
                agent_args: vec![],
                goal: "timeout".into(),
                cwd,
                timeout: Some(Duration::from_millis(50)),
                input_file: None,
            },
            &CancellationHandle::new(),
            stdout.writer(),
            SharedBuffer::default().writer(),
        )
        .expect_err("run should fail");

        let events = parse_json_lines(&stdout.contents());
        assert_eq!(
            events.last().expect("failed event")["event"],
            "spawn_failed"
        );
        assert_eq!(
            events.last().expect("failed event")["data"]["reason"],
            "child process timed out after 50ms"
        );
        assert!(matches!(error, RuntimeError::ChildTimedOut { .. }));
    }

    #[cfg(unix)]
    #[test]
    fn run_emits_failed_event_for_manual_cancellation() {
        let cwd = create_temp_dir("runtime-cancel");
        let script = create_script(
            "sleep.sh",
            r#"#!/bin/sh
sleep 5
"#,
        );

        let stdout = SharedBuffer::default();
        let cancellation = CancellationHandle::new();
        let cancellation_clone = cancellation.clone();
        thread::spawn(move || {
            thread::sleep(Duration::from_millis(50));
            cancellation_clone.cancel_with_reason("cancelled by test");
        });

        let error = run_with_io(
            RunRequest {
                agent: script.to_string_lossy().into_owned(),
                agent_args: vec![],
                goal: "cancel".into(),
                cwd,
                timeout: Some(Duration::from_millis(5_000)),
                input_file: None,
            },
            &cancellation,
            stdout.writer(),
            SharedBuffer::default().writer(),
        )
        .expect_err("run should fail");

        let events = parse_json_lines(&stdout.contents());
        assert_eq!(
            events.last().expect("failed event")["event"],
            "spawn_failed"
        );
        assert_eq!(
            events.last().expect("failed event")["data"]["reason"],
            "cancelled by test"
        );
        assert!(matches!(error, RuntimeError::ChildCancelled { .. }));
    }

    #[test]
    fn run_emits_failed_event_for_missing_directory() {
        let stdout = SharedBuffer::default();
        let error = run_with_io(
            RunRequest {
                agent: "codex".into(),
                agent_args: vec![],
                goal: "implement parser".into(),
                cwd: PathBuf::from("/definitely/not/a/real/path"),
                timeout: Some(Duration::from_millis(5_000)),
                input_file: None,
            },
            &CancellationHandle::new(),
            stdout.writer(),
            SharedBuffer::default().writer(),
        )
        .expect_err("run should fail");

        let events = parse_json_lines(&stdout.contents());
        assert_eq!(events.len(), 1);
        assert_eq!(events[0]["event"], "spawn_failed");
        assert!(matches!(error, RuntimeError::CwdDoesNotExist(_)));
    }

    #[test]
    fn run_emits_failed_event_for_missing_input_before_spawn() {
        let cwd = create_temp_dir("runtime-missing-input");
        let stdout = SharedBuffer::default();
        let stderr = SharedBuffer::default();

        let error = run_with_io(
            RunRequest {
                agent: "/bin/sh".into(),
                agent_args: vec!["-c".into(), "sleep 5".into()],
                goal: "missing input".into(),
                cwd,
                timeout: Some(Duration::from_millis(5_000)),
                input_file: Some(PathBuf::from("/definitely/not/a/real/input.jsonl")),
            },
            &CancellationHandle::new(),
            stdout.writer(),
            stderr.writer(),
        )
        .expect_err("run should fail");

        let events = parse_json_lines(&stdout.contents());
        assert_eq!(events.len(), 1);
        assert_eq!(events[0]["event"], "spawn_failed");
        assert_eq!(stderr.contents(), "");
        assert!(matches!(error, RuntimeError::InputOpenFailed { .. }));
    }

    #[test]
    fn child_process_env_contains_goal_and_run_fields() {
        let run = RunContext {
            run_id: "run-1".into(),
            parent_run_id: Some("run-root".into()),
            spawn_id: "spawn-1".into(),
        };

        let env = super::child_process_env("ship it", &run);

        assert!(env.contains(&(ACP_SPAWN_GOAL_ENV.to_string(), "ship it".into())));
        assert!(env.contains(&(RUN_ID_ENV.to_string(), "run-1".into())));
        assert!(env.contains(&(PARENT_RUN_ID_ENV.to_string(), "run-root".into())));
    }

    #[test]
    fn try_from_requires_agent_after_config_merge() {
        let error = RunRequest::try_from(crate::cli::RunArgs {
            config: None,
            profile: None,
            agent: None,
            agent_args: vec![],
            goal: Some("demo".into()),
            cwd: Some(PathBuf::from(".")),
            timeout_ms: None,
            input_file: None,
        })
        .expect_err("request should fail");

        assert!(matches!(error, RuntimeError::MissingRunField("agent")));
    }

    #[cfg(unix)]
    fn create_script(name: &str, contents: &str) -> PathBuf {
        let path = create_temp_dir("runtime-scripts").join(name);
        fs::write(&path, contents).expect("script should be written");

        let mut permissions = fs::metadata(&path)
            .expect("script metadata should exist")
            .permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&path, permissions).expect("script should be executable");

        path
    }

    fn create_temp_dir(prefix: &str) -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time should be available")
            .as_nanos();
        let path = env::temp_dir().join(format!("acp-spawn-{prefix}-{unique}"));
        fs::create_dir_all(&path).expect("temp dir should be created");
        path
    }

    fn parse_json_lines(input: &str) -> Vec<Value> {
        input
            .lines()
            .map(|line| serde_json::from_str::<Value>(line).expect("line should be valid json"))
            .collect()
    }

    #[derive(Clone, Default)]
    struct SharedBuffer {
        inner: Arc<Mutex<Vec<u8>>>,
    }

    impl SharedBuffer {
        fn writer(&self) -> SharedWriter {
            SharedWriter {
                inner: Arc::clone(&self.inner),
            }
        }

        fn contents(&self) -> String {
            String::from_utf8(
                self.inner
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner())
                    .clone(),
            )
            .expect("buffer should be utf-8")
        }
    }

    struct SharedWriter {
        inner: Arc<Mutex<Vec<u8>>>,
    }

    impl Write for SharedWriter {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            self.inner
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .extend_from_slice(buf);
            Ok(buf.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }
}
