//! Loads spawn target definitions from TOML configuration files.

use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};

use serde::Deserialize;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunConfig {
    pub agent: String,
    pub agent_args: Vec<String>,
    pub goal: String,
    pub cwd: PathBuf,
    pub timeout_ms: Option<u64>,
}

#[derive(Debug)]
pub enum ConfigError {
    MissingCurrentDirectory,
    ReadFailed { path: PathBuf, reason: String },
    ParseFailed { path: PathBuf, reason: String },
    MissingDefaultRun { path: PathBuf },
    MissingProfile { path: PathBuf, profile: String },
}

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingCurrentDirectory => {
                write!(f, "failed to determine current working directory")
            }
            Self::ReadFailed { path, reason } => {
                write!(
                    f,
                    "failed to read config file '{}': {reason}",
                    path.display()
                )
            }
            Self::ParseFailed { path, reason } => {
                write!(
                    f,
                    "failed to parse config file '{}': {reason}",
                    path.display()
                )
            }
            Self::MissingDefaultRun { path } => {
                write!(
                    f,
                    "config file '{}' does not contain a [run] section",
                    path.display()
                )
            }
            Self::MissingProfile { path, profile } => {
                write!(
                    f,
                    "config file '{}' does not contain profile '{}'",
                    path.display(),
                    profile
                )
            }
        }
    }
}

impl std::error::Error for ConfigError {}

pub fn load_run_config(
    config_path: &Path,
    profile: Option<&str>,
) -> Result<RunConfig, ConfigError> {
    let resolved_path = resolve_path(config_path)?;
    let raw = fs::read_to_string(&resolved_path).map_err(|error| ConfigError::ReadFailed {
        path: resolved_path.clone(),
        reason: error.to_string(),
    })?;
    let file = toml::from_str::<ConfigFile>(&raw).map_err(|error| ConfigError::ParseFailed {
        path: resolved_path.clone(),
        reason: error.to_string(),
    })?;
    let selected = match profile {
        Some(name) => file
            .profiles
            .as_ref()
            .and_then(|profiles| profiles.get(name))
            .cloned()
            .ok_or_else(|| ConfigError::MissingProfile {
                path: resolved_path.clone(),
                profile: name.to_string(),
            })?,
        None => file.run.ok_or_else(|| ConfigError::MissingDefaultRun {
            path: resolved_path.clone(),
        })?,
    };

    Ok(RunConfig {
        agent: selected.agent,
        agent_args: selected.agent_args.unwrap_or_default(),
        goal: selected.goal,
        cwd: resolve_config_cwd(&resolved_path, &selected.cwd)?,
        timeout_ms: selected.timeout_ms,
    })
}

fn resolve_path(path: &Path) -> Result<PathBuf, ConfigError> {
    if path.is_absolute() {
        return Ok(path.to_path_buf());
    }

    let cwd = std::env::current_dir().map_err(|_| ConfigError::MissingCurrentDirectory)?;
    Ok(cwd.join(path))
}

fn resolve_config_cwd(config_path: &Path, cwd: &Path) -> Result<PathBuf, ConfigError> {
    if cwd.is_absolute() {
        return Ok(cwd.to_path_buf());
    }

    let parent = config_path
        .parent()
        .ok_or(ConfigError::MissingCurrentDirectory)?;
    Ok(parent.join(cwd))
}

#[derive(Debug, Deserialize)]
struct ConfigFile {
    run: Option<ConfigEntry>,
    profiles: Option<std::collections::BTreeMap<String, ConfigEntry>>,
}

#[derive(Debug, Clone, Deserialize)]
struct ConfigEntry {
    agent: String,
    #[serde(default)]
    agent_args: Option<Vec<String>>,
    goal: String,
    cwd: PathBuf,
    timeout_ms: Option<u64>,
}

#[cfg(test)]
mod tests {
    use std::env;
    use std::fs;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::{ConfigError, load_run_config};

    #[test]
    fn loads_default_run_section() {
        let dir = create_temp_dir("config-default");
        let config_path = dir.join("spawn.toml");
        fs::write(
            &config_path,
            r#"
[run]
agent = "echo"
agent_args = ["hello"]
goal = "demo"
cwd = "."
timeout_ms = 1000
"#,
        )
        .expect("config should be written");

        let config = load_run_config(&config_path, None).expect("config should load");

        assert_eq!(config.agent, "echo");
        assert_eq!(config.agent_args, vec!["hello"]);
        assert_eq!(config.goal, "demo");
        assert_eq!(config.cwd, dir);
        assert_eq!(config.timeout_ms, Some(1000));
    }

    #[test]
    fn loads_named_profile() {
        let dir = create_temp_dir("config-profile");
        let config_path = dir.join("spawn.toml");
        fs::write(
            &config_path,
            r#"
[profiles.opencode-acp]
agent = "opencode"
agent_args = ["acp"]
goal = "serve acp"
cwd = "."
timeout_ms = 3000
"#,
        )
        .expect("config should be written");

        let config =
            load_run_config(&config_path, Some("opencode-acp")).expect("config should load");

        assert_eq!(config.agent, "opencode");
        assert_eq!(config.agent_args, vec!["acp"]);
        assert_eq!(config.cwd, dir);
    }

    #[test]
    fn reports_missing_profile() {
        let dir = create_temp_dir("config-missing-profile");
        let config_path = dir.join("spawn.toml");
        fs::write(
            &config_path,
            r#"
[run]
agent = "echo"
goal = "demo"
cwd = "."
"#,
        )
        .expect("config should be written");

        let error = load_run_config(&config_path, Some("missing")).expect_err("config should fail");

        assert!(matches!(error, ConfigError::MissingProfile { .. }));
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
