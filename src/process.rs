//! Spawns child processes and streams stdin, stdout, and stderr.

use std::ffi::OsString;
use std::fmt;
use std::io::{BufReader, Read, Write};
#[cfg(unix)]
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, ExitStatus, Stdio};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use crate::cancel::CancellationHandle;

const DEFAULT_POLL_INTERVAL: Duration = Duration::from_millis(50);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcessSpec {
    pub program: OsString,
    pub args: Vec<OsString>,
    pub cwd: PathBuf,
    pub env: Vec<(String, String)>,
    pub timeout: Option<Duration>,
    pub termination_grace_period: Duration,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcessOutput {
    pub success: bool,
    pub exit_code: Option<i32>,
    pub termination: ProcessTermination,
    pub duration: Duration,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProcessTermination {
    Exited,
    TimedOut,
    Cancelled { reason: String },
}

#[derive(Debug)]
pub struct RunningProcess {
    child: Child,
    stdin: Option<ChildStdin>,
    receiver: Receiver<StreamMessage>,
    stdout_handle: Option<JoinHandle<()>>,
    stderr_handle: Option<JoinHandle<()>>,
    stdin_forward_handle: Option<JoinHandle<()>>,
    timeout: Option<Duration>,
    termination_grace_period: Duration,
    poll_interval: Duration,
    started_at: Instant,
    program: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProcessError {
    WorkingDirectoryDoesNotExist(PathBuf),
    WorkingDirectoryIsNotDirectory(PathBuf),
    SpawnFailed {
        program: String,
        reason: String,
    },
    WaitFailed {
        program: String,
        reason: String,
    },
    SignalFailed {
        program: String,
        reason: String,
    },
    KillFailed {
        program: String,
        reason: String,
    },
    ReaderFailed {
        stream: &'static str,
        reason: String,
    },
    ObserverFailed {
        stream: &'static str,
        reason: String,
    },
    MissingPipe {
        stream: &'static str,
    },
    JoinFailed {
        stream: &'static str,
    },
}

impl fmt::Display for ProcessError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::WorkingDirectoryDoesNotExist(path) => {
                write!(f, "working directory does not exist: {}", path.display())
            }
            Self::WorkingDirectoryIsNotDirectory(path) => {
                write!(
                    f,
                    "working directory is not a directory: {}",
                    path.display()
                )
            }
            Self::SpawnFailed { program, reason } => {
                write!(f, "failed to spawn child process '{program}': {reason}")
            }
            Self::WaitFailed { program, reason } => {
                write!(
                    f,
                    "failed while waiting for child process '{program}': {reason}"
                )
            }
            Self::SignalFailed { program, reason } => {
                write!(f, "failed to signal child process '{program}': {reason}")
            }
            Self::KillFailed { program, reason } => {
                write!(
                    f,
                    "failed to force-kill child process '{program}': {reason}"
                )
            }
            Self::ReaderFailed { stream, reason } => {
                write!(f, "failed to read child {stream}: {reason}")
            }
            Self::ObserverFailed { stream, reason } => {
                write!(f, "failed to consume child {stream}: {reason}")
            }
            Self::MissingPipe { stream } => write!(f, "child {stream} pipe was not available"),
            Self::JoinFailed { stream } => write!(f, "child {stream} reader thread panicked"),
        }
    }
}

impl std::error::Error for ProcessError {}

pub fn spawn(spec: &ProcessSpec) -> Result<RunningProcess, ProcessError> {
    validate_cwd(&spec.cwd)?;

    let program = spec.program.to_string_lossy().into_owned();
    let mut command = Command::new(&spec.program);
    command.args(&spec.args);
    command.current_dir(&spec.cwd);
    command.envs(
        spec.env
            .iter()
            .map(|(key, value)| (key.as_str(), value.as_str())),
    );
    command.stdin(Stdio::piped());
    command.stdout(Stdio::piped());
    command.stderr(Stdio::piped());
    #[cfg(unix)]
    unsafe {
        command.pre_exec(|| {
            nix::unistd::setpgid(nix::unistd::Pid::from_raw(0), nix::unistd::Pid::from_raw(0))
                .map_err(std::io::Error::other)
        });
    }

    let mut child = command.spawn().map_err(|error| ProcessError::SpawnFailed {
        program: program.clone(),
        reason: error.to_string(),
    })?;

    let stdin = child.stdin.take();
    let stdout = child
        .stdout
        .take()
        .ok_or(ProcessError::MissingPipe { stream: "stdout" })?;
    let stderr = child
        .stderr
        .take()
        .ok_or(ProcessError::MissingPipe { stream: "stderr" })?;

    let (tx, rx) = mpsc::channel();
    let stdout_handle = spawn_reader(stdout, StreamKind::Stdout, tx.clone());
    let stderr_handle = spawn_reader(stderr, StreamKind::Stderr, tx);

    Ok(RunningProcess {
        child,
        stdin,
        receiver: rx,
        stdout_handle: Some(stdout_handle),
        stderr_handle: Some(stderr_handle),
        stdin_forward_handle: None,
        timeout: spec.timeout,
        termination_grace_period: spec.termination_grace_period,
        poll_interval: DEFAULT_POLL_INTERVAL,
        started_at: Instant::now(),
        program,
    })
}

impl RunningProcess {
    pub fn pid(&self) -> u32 {
        self.child.id()
    }

    pub fn start_stdin_forwarder<R: Read + Send + 'static>(&mut self, input: R) {
        let Some(mut child_stdin) = self.stdin.take() else {
            return;
        };

        self.stdin_forward_handle = Some(thread::spawn(move || {
            let mut reader = BufReader::new(input);
            let mut buffer = [0_u8; 4096];

            loop {
                match reader.read(&mut buffer) {
                    Ok(0) => break,
                    Ok(size) => {
                        if child_stdin.write_all(&buffer[..size]).is_err() {
                            break;
                        }
                        if child_stdin.flush().is_err() {
                            break;
                        }
                    }
                    Err(_) => break,
                }
            }
        }));
    }

    pub fn wait_with_streaming(
        mut self,
        cancellation: &CancellationHandle,
        on_stdout_chunk: &mut dyn FnMut(&[u8]) -> Result<(), String>,
        on_stderr_chunk: &mut dyn FnMut(&[u8]) -> Result<(), String>,
    ) -> Result<ProcessOutput, ProcessError> {
        let mut stdout_closed = false;
        let mut stderr_closed = false;
        let mut status: Option<ExitStatus> = None;
        let mut forced_termination: Option<ProcessTermination> = None;

        loop {
            if status.is_none() {
                if forced_termination.is_none() {
                    if let Some(timeout) = self.timeout {
                        if self.started_at.elapsed() >= timeout {
                            self.terminate_child()?;
                            forced_termination = Some(ProcessTermination::TimedOut);
                        }
                    }
                }

                if forced_termination.is_none() && cancellation.is_cancelled() {
                    self.terminate_child()?;
                    forced_termination = Some(ProcessTermination::Cancelled {
                        reason: cancellation
                            .reason()
                            .unwrap_or_else(|| "cancelled".to_string()),
                    });
                }

                status = self.try_wait()?;
            }

            if status.is_some() && stdout_closed && stderr_closed {
                break;
            }

            match self.receiver.recv_timeout(self.poll_interval) {
                Ok(StreamMessage::Chunk(StreamKind::Stdout, chunk)) => on_stdout_chunk(&chunk)
                    .map_err(|reason| ProcessError::ObserverFailed {
                        stream: "stdout",
                        reason,
                    })?,
                Ok(StreamMessage::Chunk(StreamKind::Stderr, chunk)) => on_stderr_chunk(&chunk)
                    .map_err(|reason| ProcessError::ObserverFailed {
                        stream: "stderr",
                        reason,
                    })?,
                Ok(StreamMessage::Closed(StreamKind::Stdout)) => stdout_closed = true,
                Ok(StreamMessage::Closed(StreamKind::Stderr)) => stderr_closed = true,
                Ok(StreamMessage::ReadError(kind, reason)) => {
                    return Err(ProcessError::ReaderFailed {
                        stream: kind.as_str(),
                        reason,
                    });
                }
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => {
                    stdout_closed = true;
                    stderr_closed = true;
                }
            }
        }

        self.join_readers()?;
        self.join_stdin_forwarder()?;

        let exit_status =
            status.unwrap_or_else(|| self.child.wait().expect("child should be waitable"));
        let termination = forced_termination.unwrap_or(ProcessTermination::Exited);

        Ok(ProcessOutput {
            success: exit_status.success() && matches!(termination, ProcessTermination::Exited),
            exit_code: exit_status.code(),
            termination,
            duration: self.started_at.elapsed(),
        })
    }

    fn try_wait(&mut self) -> Result<Option<ExitStatus>, ProcessError> {
        self.child
            .try_wait()
            .map_err(|error| ProcessError::WaitFailed {
                program: self.program.clone(),
                reason: error.to_string(),
            })
    }

    fn terminate_child(&mut self) -> Result<(), ProcessError> {
        if self.try_wait()?.is_some() {
            return Ok(());
        }

        send_termination_signal(&mut self.child, &self.program)?;

        let deadline = Instant::now() + self.termination_grace_period;
        while Instant::now() < deadline {
            if self.try_wait()?.is_some() {
                return Ok(());
            }
            thread::sleep(self.poll_interval);
        }

        match kill_child_group(&mut self.child, &self.program) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::InvalidInput => Ok(()),
            Err(error) => Err(ProcessError::KillFailed {
                program: self.program.clone(),
                reason: error.to_string(),
            }),
        }
    }

    fn join_readers(&mut self) -> Result<(), ProcessError> {
        self.stdout_handle
            .take()
            .expect("stdout handle should exist")
            .join()
            .map_err(|_| ProcessError::JoinFailed { stream: "stdout" })?;
        self.stderr_handle
            .take()
            .expect("stderr handle should exist")
            .join()
            .map_err(|_| ProcessError::JoinFailed { stream: "stderr" })?;
        Ok(())
    }

    fn join_stdin_forwarder(&mut self) -> Result<(), ProcessError> {
        if let Some(handle) = self.stdin_forward_handle.take() {
            handle
                .join()
                .map_err(|_| ProcessError::JoinFailed { stream: "stdin" })?;
        }
        Ok(())
    }
}

#[derive(Debug)]
enum StreamMessage {
    Chunk(StreamKind, Vec<u8>),
    Closed(StreamKind),
    ReadError(StreamKind, String),
}

#[derive(Debug, Clone, Copy)]
enum StreamKind {
    Stdout,
    Stderr,
}

impl StreamKind {
    fn as_str(self) -> &'static str {
        match self {
            Self::Stdout => "stdout",
            Self::Stderr => "stderr",
        }
    }
}

fn spawn_reader<R: Read + Send + 'static>(
    reader: R,
    kind: StreamKind,
    sender: Sender<StreamMessage>,
) -> JoinHandle<()> {
    thread::spawn(move || {
        let mut reader = BufReader::new(reader);
        let mut buffer = [0_u8; 4096];

        loop {
            match reader.read(&mut buffer) {
                Ok(0) => break,
                Ok(size) => {
                    if sender
                        .send(StreamMessage::Chunk(kind, buffer[..size].to_vec()))
                        .is_err()
                    {
                        return;
                    }
                }
                Err(error) => {
                    let _ = sender.send(StreamMessage::ReadError(kind, error.to_string()));
                    return;
                }
            }
        }

        let _ = sender.send(StreamMessage::Closed(kind));
    })
}

#[cfg(unix)]
fn send_termination_signal(child: &mut Child, program: &str) -> Result<(), ProcessError> {
    send_signal_to_child_group(child, program, nix::sys::signal::Signal::SIGTERM)
}

#[cfg(unix)]
fn kill_child_group(child: &mut Child, program: &str) -> std::io::Result<()> {
    send_signal_to_child_group(child, program, nix::sys::signal::Signal::SIGKILL)
        .map_err(|error| std::io::Error::other(error.to_string()))
}

#[cfg(unix)]
fn send_signal_to_child_group(
    child: &mut Child,
    program: &str,
    signal: nix::sys::signal::Signal,
) -> Result<(), ProcessError> {
    use nix::errno::Errno;
    use nix::sys::signal::kill;
    use nix::unistd::Pid;

    let process_group = Pid::from_raw(-(child.id() as i32));
    match kill(process_group, signal) {
        Ok(()) => Ok(()),
        Err(Errno::ESRCH) => Ok(()),
        Err(error) => Err(ProcessError::SignalFailed {
            program: program.to_string(),
            reason: error.to_string(),
        }),
    }
}

#[cfg(not(unix))]
fn send_termination_signal(child: &mut Child, program: &str) -> Result<(), ProcessError> {
    child.kill().map_err(|error| ProcessError::SignalFailed {
        program: program.to_string(),
        reason: error.to_string(),
    })
}

fn validate_cwd(cwd: &Path) -> Result<(), ProcessError> {
    if !cwd.exists() {
        return Err(ProcessError::WorkingDirectoryDoesNotExist(
            cwd.to_path_buf(),
        ));
    }

    if !cwd.is_dir() {
        return Err(ProcessError::WorkingDirectoryIsNotDirectory(
            cwd.to_path_buf(),
        ));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use std::env;
    use std::ffi::OsString;
    use std::fs;
    use std::path::PathBuf;
    use std::sync::{Arc, Mutex};
    use std::thread;
    use std::time::{Duration, SystemTime, UNIX_EPOCH};

    use crate::cancel::CancellationHandle;

    use super::{ProcessError, ProcessSpec, ProcessTermination, spawn};

    #[cfg(unix)]
    use nix::errno::Errno;
    #[cfg(unix)]
    use nix::sys::signal::kill;
    #[cfg(unix)]
    use nix::unistd::Pid;
    #[cfg(unix)]
    use std::os::unix::fs::PermissionsExt;

    fn base_spec(program: OsString, cwd: PathBuf) -> ProcessSpec {
        ProcessSpec {
            program,
            args: vec![],
            cwd,
            env: vec![],
            timeout: None,
            termination_grace_period: Duration::from_millis(100),
        }
    }

    #[test]
    fn spawn_rejects_missing_working_directory() {
        let error = spawn(&base_spec(
            OsString::from("/bin/sh"),
            PathBuf::from("/definitely/not/a/real/path"),
        ))
        .expect_err("spawn should fail");

        assert_eq!(
            error,
            ProcessError::WorkingDirectoryDoesNotExist(PathBuf::from(
                "/definitely/not/a/real/path"
            ))
        );
    }

    #[cfg(unix)]
    #[test]
    fn process_streams_stdout_and_stderr() {
        let cwd = create_temp_dir("process-success");
        let script = create_script(
            "emit-context.sh",
            r#"#!/bin/sh
printf 'line-one\n'
printf 'line-two\n'
printf 'stderr-marker\n' >&2
"#,
        );

        let mut stdout_bytes = Vec::new();
        let mut stderr_bytes = Vec::new();
        let cancellation = CancellationHandle::new();
        let running = spawn(&base_spec(script.into(), cwd)).expect("spawn should succeed");
        let output = running
            .wait_with_streaming(
                &cancellation,
                &mut |chunk| {
                    stdout_bytes.extend_from_slice(chunk);
                    Ok(())
                },
                &mut |chunk| {
                    stderr_bytes.extend_from_slice(chunk);
                    Ok(())
                },
            )
            .expect("wait should succeed");

        assert!(output.success);
        assert_eq!(output.exit_code, Some(0));
        assert_eq!(output.termination, ProcessTermination::Exited);
        assert_eq!(
            String::from_utf8(stdout_bytes).unwrap(),
            "line-one\nline-two\n"
        );
        assert_eq!(String::from_utf8(stderr_bytes).unwrap(), "stderr-marker\n");
    }

    #[cfg(unix)]
    #[test]
    fn process_times_out() {
        let cwd = create_temp_dir("process-timeout");
        let script = create_script(
            "sleep.sh",
            r#"#!/bin/sh
sleep 5
"#,
        );

        let mut spec = base_spec(script.into(), cwd);
        spec.timeout = Some(Duration::from_millis(50));

        let cancellation = CancellationHandle::new();
        let running = spawn(&spec).expect("spawn should succeed");
        let output = running
            .wait_with_streaming(&cancellation, &mut |_| Ok(()), &mut |_| Ok(()))
            .expect("wait should finish");

        assert!(!output.success);
        assert_eq!(output.termination, ProcessTermination::TimedOut);
    }

    #[cfg(unix)]
    #[test]
    fn process_forwards_stdin_to_child() {
        let cwd = create_temp_dir("process-stdin");
        let script = create_script(
            "read-stdin.sh",
            r#"#!/bin/sh
IFS= read -r line
printf '{"event":"stdin_echo","value":"%s"}\n' "$line"
"#,
        );

        let cancellation = CancellationHandle::new();
        let mut stdout_bytes = Vec::new();
        let mut running = spawn(&base_spec(script.into(), cwd)).expect("spawn should succeed");
        running.start_stdin_forwarder(std::io::Cursor::new(b"hello-from-stdin\n".to_vec()));

        let output = running
            .wait_with_streaming(
                &cancellation,
                &mut |chunk| {
                    stdout_bytes.extend_from_slice(chunk);
                    Ok(())
                },
                &mut |_| Ok(()),
            )
            .expect("wait should succeed");

        assert!(output.success);
        assert_eq!(
            String::from_utf8(stdout_bytes).unwrap(),
            "{\"event\":\"stdin_echo\",\"value\":\"hello-from-stdin\"}\n"
        );
    }

    #[cfg(unix)]
    #[test]
    fn process_cancels_when_requested() {
        let cwd = create_temp_dir("process-cancel");
        let script = create_script(
            "sleep.sh",
            r#"#!/bin/sh
sleep 5
"#,
        );

        let cancellation = CancellationHandle::new();
        let cancellation_clone = cancellation.clone();
        let running = spawn(&base_spec(script.into(), cwd)).expect("spawn should succeed");

        thread::spawn(move || {
            thread::sleep(Duration::from_millis(50));
            cancellation_clone.cancel_with_reason("cancelled by test");
        });

        let output = running
            .wait_with_streaming(&cancellation, &mut |_| Ok(()), &mut |_| Ok(()))
            .expect("wait should finish");

        assert!(!output.success);
        assert_eq!(
            output.termination,
            ProcessTermination::Cancelled {
                reason: "cancelled by test".into(),
            }
        );
    }

    #[cfg(unix)]
    #[test]
    fn process_preserves_stdout_without_trailing_newline() {
        let cwd = create_temp_dir("process-stdout-raw");
        let script = create_script(
            "no-newline.sh",
            r#"#!/bin/sh
printf 'no-newline'
"#,
        );

        let cancellation = CancellationHandle::new();
        let mut stdout_bytes = Vec::new();
        let running = spawn(&base_spec(script.into(), cwd)).expect("spawn should succeed");
        let output = running
            .wait_with_streaming(
                &cancellation,
                &mut |chunk| {
                    stdout_bytes.extend_from_slice(chunk);
                    Ok(())
                },
                &mut |_| Ok(()),
            )
            .expect("wait should succeed");

        assert!(output.success);
        assert_eq!(stdout_bytes, b"no-newline");
    }

    #[cfg(unix)]
    #[test]
    fn process_cancellation_terminates_detached_children_in_group() {
        let cwd = create_temp_dir("process-cancel-group");
        let script = create_script(
            "group-cancel.sh",
            r#"#!/bin/sh
sleep 30 &
printf '%s\n' "$!"
wait
"#,
        );

        let cancellation = CancellationHandle::new();
        let cancellation_clone = cancellation.clone();
        let spec = base_spec(script.into(), cwd);
        let running = spawn(&spec).expect("spawn should succeed");
        let grandchild_pid = Arc::new(Mutex::new(None));
        let grandchild_pid_for_callback = Arc::clone(&grandchild_pid);
        let output = running
            .wait_with_streaming(
                &cancellation,
                &mut |chunk| {
                    let pid = String::from_utf8_lossy(chunk)
                        .trim()
                        .parse::<i32>()
                        .map_err(|error| error.to_string())?;
                    *grandchild_pid_for_callback
                        .lock()
                        .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(pid);
                    cancellation_clone.cancel_with_reason("cancelled after child group started");
                    Ok(())
                },
                &mut |_| Ok(()),
            )
            .expect("wait should finish");

        assert_eq!(
            output.termination,
            ProcessTermination::Cancelled {
                reason: "cancelled after child group started".into(),
            }
        );

        let grandchild_pid = grandchild_pid
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .expect("grandchild pid should be captured");
        assert!(
            matches!(kill(Pid::from_raw(grandchild_pid), None), Err(Errno::ESRCH)),
            "grandchild should not still be alive after timeout"
        );
    }

    #[cfg(unix)]
    fn create_script(name: &str, contents: &str) -> PathBuf {
        let path = create_temp_dir("process-scripts").join(name);
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
}
