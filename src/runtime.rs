use std::fmt;
use std::path::{Path, PathBuf};

use crate::cli::RunArgs;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunRequest {
    pub agent: String,
    pub goal: String,
    pub cwd: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunOutcome {
    pub summary: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuntimeError {
    MissingCurrentDirectory,
    CwdDoesNotExist(PathBuf),
    CwdIsNotDirectory(PathBuf),
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
        }
    }
}

impl std::error::Error for RuntimeError {}

impl TryFrom<RunArgs> for RunRequest {
    type Error = RuntimeError;

    fn try_from(args: RunArgs) -> Result<Self, Self::Error> {
        let cwd = resolve_cwd(&args.cwd)?;
        validate_cwd(&cwd)?;

        Ok(Self {
            agent: args.agent,
            goal: args.goal,
            cwd,
        })
    }
}

pub fn run(request: RunRequest) -> Result<RunOutcome, RuntimeError> {
    validate_cwd(&request.cwd)?;

    Ok(RunOutcome {
        summary: format!(
            "initialized spawn runtime for agent '{}' with goal '{}' in {}",
            request.agent,
            request.goal,
            request.cwd.display()
        ),
    })
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

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::{RunRequest, RuntimeError, run};

    #[test]
    fn run_succeeds_with_existing_directory() {
        let request = RunRequest {
            agent: "codex".into(),
            goal: "implement parser".into(),
            cwd: std::env::temp_dir(),
        };

        let outcome = run(request).expect("run should succeed");

        assert!(outcome.summary.contains("initialized spawn runtime"));
    }

    #[test]
    fn run_rejects_missing_directory() {
        let request = RunRequest {
            agent: "codex".into(),
            goal: "implement parser".into(),
            cwd: PathBuf::from("/definitely/not/a/real/path"),
        };

        let error = run(request).expect_err("run should fail");

        assert_eq!(
            error,
            RuntimeError::CwdDoesNotExist(PathBuf::from("/definitely/not/a/real/path"))
        );
    }
}
