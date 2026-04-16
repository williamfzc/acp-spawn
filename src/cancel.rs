//! Tracks cancellation requests and process-level termination signals.

use std::fmt;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

#[derive(Clone, Debug, Default)]
pub struct CancellationHandle {
    cancelled: Arc<AtomicBool>,
    signal: Arc<AtomicUsize>,
    manual_reason: Arc<Mutex<Option<String>>>,
}

impl CancellationHandle {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn install_signal_handlers() -> Result<Self, CancelError> {
        let handle = Self::new();

        #[cfg(unix)]
        {
            use signal_hook::consts::{SIGINT, SIGTERM};

            signal_hook::flag::register(SIGTERM, Arc::clone(&handle.cancelled))
                .map_err(CancelError::SignalRegistration)?;
            signal_hook::flag::register_usize(
                SIGTERM,
                Arc::clone(&handle.signal),
                SIGTERM as usize,
            )
            .map_err(CancelError::SignalRegistration)?;
            signal_hook::flag::register(SIGINT, Arc::clone(&handle.cancelled))
                .map_err(CancelError::SignalRegistration)?;
            signal_hook::flag::register_usize(SIGINT, Arc::clone(&handle.signal), SIGINT as usize)
                .map_err(CancelError::SignalRegistration)?;
        }

        Ok(handle)
    }

    pub fn cancel_with_reason(&self, reason: impl Into<String>) {
        let mut slot = self
            .manual_reason
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        *slot = Some(reason.into());
        self.cancelled.store(true, Ordering::Relaxed);
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Relaxed)
    }

    pub fn reason(&self) -> Option<String> {
        if let Some(reason) = self
            .manual_reason
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
        {
            return Some(reason);
        }

        match self.signal.load(Ordering::Relaxed) {
            #[cfg(unix)]
            value if value == signal_hook::consts::SIGTERM as usize => {
                Some("received SIGTERM".to_string())
            }
            #[cfg(unix)]
            value if value == signal_hook::consts::SIGINT as usize => {
                Some("received SIGINT".to_string())
            }
            _ => None,
        }
    }
}

#[derive(Debug)]
pub enum CancelError {
    SignalRegistration(std::io::Error),
}

impl fmt::Display for CancelError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::SignalRegistration(error) => {
                write!(f, "failed to register signal handler: {error}")
            }
        }
    }
}

impl std::error::Error for CancelError {}

#[cfg(test)]
mod tests {
    use super::CancellationHandle;

    #[test]
    fn manual_cancel_sets_reason() {
        let handle = CancellationHandle::new();
        handle.cancel_with_reason("cancelled by test");

        assert!(handle.is_cancelled());
        assert_eq!(handle.reason().as_deref(), Some("cancelled by test"));
    }
}
