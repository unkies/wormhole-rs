use std::sync::{mpsc, Arc, Mutex};

use nix::sys::wait;
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
struct SerializableError {
    source: Option<Box<SerializableError>>,
    description: String,
}

impl SerializableError {
    pub fn new<T>(e: &T) -> SerializableError
    where
        T: ?Sized + std::error::Error,
    {
        SerializableError {
            description: e.to_string(),
            source: e.source().map(|s| Box::new(SerializableError::new(s))),
        }
    }
}

impl std::fmt::Display for SerializableError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.description)
    }
}

impl std::error::Error for SerializableError {
    fn source(&self) -> Option<&(dyn 'static + std::error::Error)> {
        self.source
            .as_ref()
            .map(|source| &**source as &(dyn 'static + std::error::Error))
    }

    fn description(&self) -> &str {
        &self.description
    }
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("failed to create channel")]
    Channel(#[from] crate::channel::Error),
    #[error("failed to fork")]
    Fork(#[source] nix::Error),
    #[error("failed to wait for child process")]
    Wait(#[source] nix::Error),
    #[error("failed to run function in child process")]
    Execution(#[source] Box<dyn std::error::Error + Send + Sync>),
    #[error("the closure caused the child process to panic: {0}")]
    Panic(String),
}

// There is no easy way to allow arbitury error type, so the best we can do is
// to use String type as a substitude.
#[derive(Debug, thiserror::Error)]
pub struct RunError(String);

impl std::fmt::Display for RunError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:#}", self.0)
    }
}

impl<T: Into<String>> From<T> for RunError {
    fn from(s: T) -> Self {
        RunError(s.into())
    }
}

/// Run the callback function in a forked process.
pub fn fork_and_run<F>(cb: F) -> Result<(), Error>
where
    F: FnOnce() -> Result<(), RunError> + std::panic::UnwindSafe,
{
    let (mut sender, mut receiver) = crate::channel::channel::<Result<(), SerializableError>>()?;
    match unsafe { nix::unistd::fork().map_err(Error::Fork)? } {
        nix::unistd::ForkResult::Parent { child } => {
            let res = receiver.recv().map_err(Error::Channel)?;
            wait::waitpid(child, None).map_err(Error::Wait)?;
            res.map_err(|err| Error::Execution(Box::new(err)))?;
        }
        nix::unistd::ForkResult::Child => {
            // Given rust ownership model, we can't shares a memory pointer for
            // the panic hook to pass the panic information to outside of the
            // panic hook. So we have to use a channel to pass the information.
            let (panic_info_tx, panic_info_rx) = mpsc::sync_channel::<String>(1);
            std::panic::set_hook(Box::new(move |info| {
                // The only possible error is if the receiver is closed, so it
                // is safe to ignore here.
                let _ = panic_info_tx.send(format!("{:?}", info));
            }));
            let test_result = match std::panic::catch_unwind(cb) {
                Ok(ret) => ret.map_err(|err| SerializableError::new(&err)),
                Err(_) => {
                    // If we can't receive the panic information, there is
                    // nothing we can do. So we set the reason to unknown and
                    // continue.
                    let reason = match panic_info_rx.try_recv() {
                        Ok(reason) => reason,
                        Err(_) => "unknown".to_string(),
                    };
                    Err(SerializableError::new(&Error::Panic(reason.clone())))
                }
            };

            // If we can't send the error to the parent process, there is
            // nothing we can do other than exit properly.
            let _ = sender.send(test_result);
            std::process::exit(0);
        }
    };

    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn test_basic() {
        crate::fork::fork_and_run(|| Ok(())).expect("failed to run");
    }

    #[test]
    fn test_error() {
        crate::fork::fork_and_run(|| {
            Err("there is an error")?;
            unreachable!()
        })
        .expect_err("should fail");

        crate::fork::fork_and_run(|| {
            Err(std::io::Error::new(
                std::io::ErrorKind::Other,
                "there is an error",
            ))
            .map_err(|err| crate::fork::RunError::from(err.to_string()))?;
            unreachable!()
        })
        .expect_err("should fail");
    }

    #[test]
    fn test_panic() {
        let err = crate::fork::fork_and_run(|| {
            panic!("there is a panic");
        })
        .expect_err("should fail");
        println!("{:#}", err);
        assert!(false);
    }
}
