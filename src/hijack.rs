//! Installs, reports, removes, and executes command hijack shims for supported agent CLIs.

use std::collections::BTreeMap;
use std::env;
use std::ffi::{OsStr, OsString};
use std::fmt;
use std::fs;
use std::io::{self, Write};
#[cfg(unix)]
use std::os::unix::fs::symlink;
#[cfg(unix)]
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use serde::{Deserialize, Serialize};

const SUPPORTED_COMMANDS: &[&str] = &["codex", "claude"];
const PATH_MARKER_START: &str = "# >>> acp-spawn hijack >>>";
const PATH_MARKER_END: &str = "# <<< acp-spawn hijack <<<";

#[derive(Debug)]
pub enum HijackError {
    MissingHomeDirectory,
    MissingCurrentExecutable,
    UnsupportedCommand(String),
    CommandNotFound(String),
    ConfigRead { path: PathBuf, reason: String },
    ConfigWrite { path: PathBuf, reason: String },
    Io { path: PathBuf, reason: String },
    ShellRcMissingParent(PathBuf),
    ProxyTargetMissing(String),
    ProxyExec { program: PathBuf, reason: String },
    StatusWrite(io::Error),
}

impl fmt::Display for HijackError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingHomeDirectory => write!(f, "failed to determine home directory"),
            Self::MissingCurrentExecutable => {
                write!(f, "failed to determine current executable path")
            }
            Self::UnsupportedCommand(command) => {
                write!(f, "unsupported command for hijack: {command}")
            }
            Self::CommandNotFound(command) => {
                write!(f, "failed to find real command in PATH: {command}")
            }
            Self::ConfigRead { path, reason } => {
                write!(
                    f,
                    "failed to read hijack config '{}': {reason}",
                    path.display()
                )
            }
            Self::ConfigWrite { path, reason } => {
                write!(
                    f,
                    "failed to write hijack config '{}': {reason}",
                    path.display()
                )
            }
            Self::Io { path, reason } => {
                write!(f, "failed to update '{}': {reason}", path.display())
            }
            Self::ShellRcMissingParent(path) => {
                write!(
                    f,
                    "shell rc file has no parent directory: {}",
                    path.display()
                )
            }
            Self::ProxyTargetMissing(command) => {
                write!(f, "no hijack target is configured for command '{command}'")
            }
            Self::ProxyExec { program, reason } => {
                write!(f, "failed to execute '{}': {reason}", program.display())
            }
            Self::StatusWrite(error) => write!(f, "failed to write status output: {error}"),
        }
    }
}

impl std::error::Error for HijackError {}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct HijackConfig {
    shim_dir: PathBuf,
    shell_rc: PathBuf,
    commands: BTreeMap<String, PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StatusEntry {
    pub command: String,
    pub configured: bool,
    pub shim_exists: bool,
    pub real_path: Option<PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ProxyInvocation {
    program: PathBuf,
    args: Vec<OsString>,
}

pub fn proxied_invocation_name(argv0: Option<OsString>) -> Option<String> {
    let name = argv0
        .as_deref()
        .map(Path::new)
        .and_then(Path::file_name)
        .and_then(OsStr::to_str)?;

    SUPPORTED_COMMANDS
        .iter()
        .find(|candidate| **candidate == name)
        .map(|name| (*name).to_string())
}

pub fn install(selected_commands: Vec<String>) -> Result<(), HijackError> {
    let paths = AppPaths::resolve()?;
    fs::create_dir_all(&paths.config_dir).map_err(|error| HijackError::Io {
        path: paths.config_dir.clone(),
        reason: error.to_string(),
    })?;
    fs::create_dir_all(&paths.shim_dir).map_err(|error| HijackError::Io {
        path: paths.shim_dir.clone(),
        reason: error.to_string(),
    })?;

    let current_exe = env::current_exe().map_err(|_| HijackError::MissingCurrentExecutable)?;
    let commands = normalize_selected_commands(selected_commands)?;
    let mut configured = BTreeMap::new();

    for command in commands {
        let real_path = find_real_command(&command, &paths.shim_dir)?
            .ok_or_else(|| HijackError::CommandNotFound(command.to_string()))?;
        create_shim(&paths.shim_dir.join(&command), &current_exe)?;
        configured.insert(command.to_string(), real_path);
    }

    let config = HijackConfig {
        shim_dir: paths.shim_dir.clone(),
        shell_rc: paths.shell_rc.clone(),
        commands: configured,
    };
    save_config(&paths.config_file, &config)?;
    ensure_shell_rc_contains_path(&paths.shell_rc, &paths.shim_dir)?;
    eprintln!(
        "Installed command hijack for: {}",
        config
            .commands
            .keys()
            .cloned()
            .collect::<Vec<_>>()
            .join(", ")
    );
    eprintln!("Shell config updated: {}", paths.shell_rc.display());
    eprintln!(
        "Open a new shell, or run: source {}",
        paths.shell_rc.display()
    );
    eprintln!("To remove this integration, run: acp-spawn uninstall");
    Ok(())
}

pub fn status<W: Write>(mut writer: W) -> Result<(), HijackError> {
    let paths = AppPaths::resolve()?;
    let config = load_config_if_present(&paths.config_file)?;
    let shell_rc = config
        .as_ref()
        .map(|config| config.shell_rc.clone())
        .unwrap_or(paths.shell_rc.clone());
    let shell_line_installed = shell_rc_contains_marker(&shell_rc)?;

    writeln!(writer, "shim_dir={}", paths.shim_dir.display()).map_err(HijackError::StatusWrite)?;
    writeln!(writer, "config_file={}", paths.config_file.display())
        .map_err(HijackError::StatusWrite)?;
    writeln!(writer, "shell_rc={}", shell_rc.display()).map_err(HijackError::StatusWrite)?;
    writeln!(writer, "shell_path_installed={shell_line_installed}")
        .map_err(HijackError::StatusWrite)?;
    writeln!(writer, "help_install=acp-spawn install").map_err(HijackError::StatusWrite)?;
    writeln!(writer, "help_uninstall=acp-spawn uninstall").map_err(HijackError::StatusWrite)?;

    for command in SUPPORTED_COMMANDS {
        let real_path = config
            .as_ref()
            .and_then(|config| config.commands.get(*command))
            .cloned();
        let shim_path = paths.shim_dir.join(command);
        let entry = StatusEntry {
            command: (*command).to_string(),
            configured: real_path.is_some(),
            shim_exists: shim_path.exists(),
            real_path,
        };
        write_status_entry(&mut writer, &entry)?;
    }

    Ok(())
}

pub fn uninstall() -> Result<(), HijackError> {
    let paths = AppPaths::resolve()?;
    let config = load_config_if_present(&paths.config_file)?;
    let shell_rc = config
        .as_ref()
        .map(|config| config.shell_rc.clone())
        .unwrap_or(paths.shell_rc.clone());

    remove_shell_rc_marker(&shell_rc)?;

    for command in SUPPORTED_COMMANDS {
        let shim_path = paths.shim_dir.join(command);
        if shim_path.exists() {
            fs::remove_file(&shim_path).map_err(|error| HijackError::Io {
                path: shim_path.clone(),
                reason: error.to_string(),
            })?;
        }
    }

    if paths.config_file.exists() {
        fs::remove_file(&paths.config_file).map_err(|error| HijackError::Io {
            path: paths.config_file.clone(),
            reason: error.to_string(),
        })?;
    }

    eprintln!("Removed acp-spawn command hijack integration.");
    eprintln!("Open a new shell to stop using the injected PATH entry.");
    eprintln!("You can verify removal with: acp-spawn status");
    Ok(())
}

pub fn proxy_invocation(command_name: &str, args: Vec<OsString>) -> Result<(), HijackError> {
    let paths = AppPaths::resolve()?;
    let configured_path = load_config_if_present(&paths.config_file)?
        .and_then(|config| config.commands.get(command_name).cloned());
    let real_path = match configured_path {
        Some(path) => path,
        None => find_real_command(command_name, &paths.shim_dir)?
            .ok_or_else(|| HijackError::ProxyTargetMissing(command_name.to_string()))?,
    };

    exec_proxy_invocation(build_proxy_invocation(real_path, args))
}

fn build_proxy_invocation(program: PathBuf, args: Vec<OsString>) -> ProxyInvocation {
    ProxyInvocation { program, args }
}

fn exec_proxy_invocation(invocation: ProxyInvocation) -> Result<(), HijackError> {
    let mut command = Command::new(&invocation.program);
    command.args(&invocation.args);
    command.stdin(Stdio::inherit());
    command.stdout(Stdio::inherit());
    command.stderr(Stdio::inherit());

    #[cfg(unix)]
    {
        let error = command.exec();
        eprintln!(
            "failed to execute '{}': {error}",
            invocation.program.display()
        );
        std::process::exit(proxy_exec_failure_code(&error));
    }

    #[cfg(not(unix))]
    {
        let status = command.status().map_err(|error| HijackError::ProxyExec {
            program: invocation.program.clone(),
            reason: error.to_string(),
        })?;
        std::process::exit(status.code().unwrap_or(1));
    }
}

#[cfg(unix)]
fn proxy_exec_failure_code(error: &io::Error) -> i32 {
    match error.kind() {
        io::ErrorKind::NotFound => 127,
        io::ErrorKind::PermissionDenied => 126,
        _ => 126,
    }
}

fn normalize_selected_commands(selected_commands: Vec<String>) -> Result<Vec<String>, HijackError> {
    if selected_commands.is_empty() {
        return Ok(SUPPORTED_COMMANDS
            .iter()
            .map(|name| (*name).to_string())
            .collect());
    }

    let mut unique = Vec::new();
    for command in selected_commands {
        if !SUPPORTED_COMMANDS
            .iter()
            .any(|supported| *supported == command)
        {
            return Err(HijackError::UnsupportedCommand(command));
        }
        if !unique.iter().any(|existing| existing == &command) {
            unique.push(command);
        }
    }
    Ok(unique)
}

fn write_status_entry<W: Write>(writer: &mut W, entry: &StatusEntry) -> Result<(), HijackError> {
    let real_path = entry
        .real_path
        .as_ref()
        .map(|path| path.display().to_string())
        .unwrap_or_else(|| "-".to_string());
    writeln!(
        writer,
        "{}: configured={} shim_exists={} real_path={}",
        entry.command, entry.configured, entry.shim_exists, real_path
    )
    .map_err(HijackError::StatusWrite)
}

fn load_config_if_present(path: &Path) -> Result<Option<HijackConfig>, HijackError> {
    if !path.exists() {
        return Ok(None);
    }

    let raw = fs::read_to_string(path).map_err(|error| HijackError::ConfigRead {
        path: path.to_path_buf(),
        reason: error.to_string(),
    })?;
    let config = toml::from_str::<HijackConfig>(&raw).map_err(|error| HijackError::ConfigRead {
        path: path.to_path_buf(),
        reason: error.to_string(),
    })?;
    Ok(Some(config))
}

fn save_config(path: &Path, config: &HijackConfig) -> Result<(), HijackError> {
    let raw = toml::to_string(config).map_err(|error| HijackError::ConfigWrite {
        path: path.to_path_buf(),
        reason: error.to_string(),
    })?;
    fs::write(path, raw).map_err(|error| HijackError::ConfigWrite {
        path: path.to_path_buf(),
        reason: error.to_string(),
    })
}

fn ensure_shell_rc_contains_path(shell_rc: &Path, shim_dir: &Path) -> Result<(), HijackError> {
    let parent = shell_rc
        .parent()
        .ok_or_else(|| HijackError::ShellRcMissingParent(shell_rc.to_path_buf()))?;
    fs::create_dir_all(parent).map_err(|error| HijackError::Io {
        path: parent.to_path_buf(),
        reason: error.to_string(),
    })?;

    let existing = if shell_rc.exists() {
        fs::read_to_string(shell_rc).map_err(|error| HijackError::Io {
            path: shell_rc.to_path_buf(),
            reason: error.to_string(),
        })?
    } else {
        String::new()
    };
    let without_marker = strip_marker_block(&existing);
    let path_block = format!(
        "{PATH_MARKER_START}\nexport PATH=\"{}:$PATH\"\n{PATH_MARKER_END}\n",
        shim_dir.display()
    );
    let mut updated = without_marker.trim_end().to_string();
    if !updated.is_empty() {
        updated.push('\n');
    }
    updated.push_str(&path_block);
    fs::write(shell_rc, updated).map_err(|error| HijackError::Io {
        path: shell_rc.to_path_buf(),
        reason: error.to_string(),
    })?;
    Ok(())
}

fn remove_shell_rc_marker(shell_rc: &Path) -> Result<(), HijackError> {
    if !shell_rc.exists() {
        return Ok(());
    }

    let existing = fs::read_to_string(shell_rc).map_err(|error| HijackError::Io {
        path: shell_rc.to_path_buf(),
        reason: error.to_string(),
    })?;
    let updated = strip_marker_block(&existing);
    fs::write(shell_rc, updated).map_err(|error| HijackError::Io {
        path: shell_rc.to_path_buf(),
        reason: error.to_string(),
    })?;
    Ok(())
}

fn shell_rc_contains_marker(shell_rc: &Path) -> Result<bool, HijackError> {
    if !shell_rc.exists() {
        return Ok(false);
    }

    let existing = fs::read_to_string(shell_rc).map_err(|error| HijackError::Io {
        path: shell_rc.to_path_buf(),
        reason: error.to_string(),
    })?;
    Ok(existing.contains(PATH_MARKER_START) && existing.contains(PATH_MARKER_END))
}

fn strip_marker_block(contents: &str) -> String {
    let mut output = Vec::new();
    let mut skipping = false;

    for line in contents.lines() {
        if line == PATH_MARKER_START {
            skipping = true;
            continue;
        }
        if line == PATH_MARKER_END {
            skipping = false;
            continue;
        }
        if !skipping {
            output.push(line);
        }
    }

    if output.is_empty() {
        String::new()
    } else {
        format!("{}\n", output.join("\n"))
    }
}

fn create_shim(shim_path: &Path, current_exe: &Path) -> Result<(), HijackError> {
    if shim_path.exists() {
        fs::remove_file(shim_path).map_err(|error| HijackError::Io {
            path: shim_path.to_path_buf(),
            reason: error.to_string(),
        })?;
    }

    #[cfg(unix)]
    {
        symlink(current_exe, shim_path).map_err(|error| HijackError::Io {
            path: shim_path.to_path_buf(),
            reason: error.to_string(),
        })?;
    }

    #[cfg(not(unix))]
    {
        fs::copy(current_exe, shim_path).map_err(|error| HijackError::Io {
            path: shim_path.to_path_buf(),
            reason: error.to_string(),
        })?;
    }

    Ok(())
}

fn find_real_command(command: &str, shim_dir: &Path) -> Result<Option<PathBuf>, HijackError> {
    let current_exe = env::current_exe().map_err(|_| HijackError::MissingCurrentExecutable)?;
    let current_exe = fs::canonicalize(current_exe).unwrap_or_else(|_| PathBuf::new());

    let path = env::var_os("PATH").unwrap_or_default();
    for entry in env::split_paths(&path) {
        if entry == shim_dir {
            continue;
        }
        let candidate = entry.join(command);
        if !candidate.is_file() {
            continue;
        }
        let canonical = fs::canonicalize(&candidate).unwrap_or_else(|_| candidate.clone());
        if !current_exe.as_os_str().is_empty() && canonical == current_exe {
            continue;
        }
        return Ok(Some(candidate));
    }

    Ok(None)
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct AppPaths {
    config_dir: PathBuf,
    config_file: PathBuf,
    shim_dir: PathBuf,
    shell_rc: PathBuf,
}

impl AppPaths {
    fn resolve() -> Result<Self, HijackError> {
        let home = env::var_os("HOME")
            .map(PathBuf::from)
            .ok_or(HijackError::MissingHomeDirectory)?;
        let config_dir = home.join(".config/acp-spawn");
        let config_file = config_dir.join("hijack.toml");
        let shim_dir = home.join(".local/share/acp-spawn/shims");
        let shell_rc = detect_shell_rc(&home);

        Ok(Self {
            config_dir,
            config_file,
            shim_dir,
            shell_rc,
        })
    }
}

fn detect_shell_rc(home: &Path) -> PathBuf {
    let shell = env::var("SHELL").unwrap_or_default();
    if shell.contains("zsh") {
        return home.join(".zshrc");
    }
    if shell.contains("bash") {
        let bash_rc = home.join(".bashrc");
        if bash_rc.exists() {
            return bash_rc;
        }
        return home.join(".bash_profile");
    }
    home.join(".profile")
}

#[cfg(test)]
mod tests {
    use std::ffi::OsString;
    use std::path::PathBuf;

    #[cfg(unix)]
    use nix::libc;

    use super::{
        SUPPORTED_COMMANDS, build_proxy_invocation, normalize_selected_commands,
        proxied_invocation_name, proxy_exec_failure_code, strip_marker_block,
    };

    #[test]
    fn recognizes_supported_proxy_name() {
        assert_eq!(
            proxied_invocation_name(Some(OsString::from("/tmp/codex"))),
            Some("codex".into())
        );
        assert_eq!(
            proxied_invocation_name(Some(OsString::from("acp-spawn"))),
            None
        );
    }

    #[test]
    fn strips_shell_marker_block() {
        let input = format!(
            "line-1\n{start}\nexport PATH=\"shim:$PATH\"\n{end}\nline-2\n",
            start = super::PATH_MARKER_START,
            end = super::PATH_MARKER_END
        );
        assert_eq!(strip_marker_block(&input), "line-1\nline-2\n");
    }

    #[test]
    fn defaults_to_supported_commands() {
        assert_eq!(
            normalize_selected_commands(vec![]).expect("commands should normalize"),
            SUPPORTED_COMMANDS
                .iter()
                .map(|name| (*name).to_string())
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn deduplicates_selected_commands() {
        assert_eq!(
            normalize_selected_commands(vec!["codex".into(), "codex".into(), "claude".into()])
                .expect("commands should normalize"),
            vec!["codex".to_string(), "claude".to_string()]
        );
    }

    #[test]
    fn proxy_invocation_preserves_real_command_and_original_args() {
        let invocation = build_proxy_invocation(
            "/usr/local/bin/codex".into(),
            vec![OsString::from("run"), OsString::from("--json")],
        );

        assert_eq!(invocation.program, PathBuf::from("/usr/local/bin/codex"));
        assert_eq!(
            invocation.args,
            vec![OsString::from("run"), OsString::from("--json")]
        );
    }

    #[cfg(unix)]
    #[test]
    fn proxy_exec_failure_codes_match_shell_conventions() {
        assert_eq!(
            proxy_exec_failure_code(&std::io::Error::from_raw_os_error(libc::ENOENT)),
            127
        );
        assert_eq!(
            proxy_exec_failure_code(&std::io::Error::from_raw_os_error(libc::EACCES)),
            126
        );
    }
}
