use std::sync::mpsc;

use nix::sys::wait;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("failed to create channel")]
    Channel(#[from] crate::channel::Error),
    #[error("failed to fork")]
    Fork(#[source] nix::Error),
    #[error("failed to wait for child process")]
    Wait(#[source] nix::Error),
    #[error("the closure caused the child process to panic: {0}")]
    Panic(String),
}

/// Run the callback function in a forked process.
pub fn fork_and_run<F>(cb: F) -> Result<(), Error>
where
    F: FnOnce() -> (),
{
    let (mut sender, mut receiver) = crate::channel::channel::<Option<String>>()?;
    match unsafe { nix::unistd::fork().map_err(Error::Fork)? } {
        nix::unistd::ForkResult::Parent { child } => {
            let res = receiver.recv().map_err(Error::Channel)?;
            wait::waitpid(child, None).map_err(Error::Wait)?;
            match res {
                Some(reason) => Err(Error::Panic(reason)),
                None => Ok(()),
            }
        }
        nix::unistd::ForkResult::Child => {
            // Given rust ownership model, we can't shares a memory pointer for
            // the panic hook to pass the panic information to outside of the
            // panic hook. So we have to use a channel to pass the information.
            let (panic_info_tx, panic_info_rx) = mpsc::sync_channel::<String>(1);
            std::panic::set_hook(Box::new(move |info| {
                // The only possible error is if the receiver is closed, so it
                // is safe to ignore here.
                let _ = panic_info_tx.send(format!("there is a panic at: {:?}", info.location()));
            }));
            // We assert the cb as unwind safe. We can do it here because we
            // will throw away the process at the end, so there is no need to
            // provide the guarantee that the closure will be able to unwind
            // properly. As long as we can return the panic information, we are
            // good.
            let did_panic = match std::panic::catch_unwind(std::panic::AssertUnwindSafe(cb)) {
                Ok(_) => None,
                Err(_) => {
                    // If we can't receive the panic information, there is
                    // nothing we can do. So we set the reason to unknown and
                    // continue.
                    let reason = match panic_info_rx.try_recv() {
                        Ok(reason) => reason,
                        Err(_) => "unknown".to_string(),
                    };
                    Some(reason)
                }
            };

            // If we can't send the error to the parent process, there is
            // nothing we can do other than exit properly.
            let _ = sender.send(did_panic);
            std::process::exit(0);
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn test_basic() {
        crate::fork::fork_and_run(|| {}).expect("failed to run");
    }

    #[test]
    fn test_panic() {
        match crate::fork::fork_and_run(|| {
            panic!("there is a panic");
        })
        .expect_err("the function should error because the closure panics")
        {
            crate::fork::Error::Panic(reason) => {
                assert_ne!(reason, "");
                assert_ne!(reason, "unknown");
            }
            _ => panic!("the error should be a panic error"),
        }
    }

    #[test]
    fn test_error() {
        // In general, rust error types are not serializable. So we can't use it
        // with the channels. Using a `Option<String>` as a cheap simulation of
        // errors. Most of the time, we can about the error message, not the
        // error type.
        let (mut sender, mut receiver) =
            crate::channel::channel::<Option<String>>().expect("failed to create channel");
        crate::fork::fork_and_run(|| {
            let _ = sender.send(Some("there is an error".to_string()));
        })
        .expect("failed to run");
        let ret = receiver.recv().expect("failed to receive error");
        assert_eq!(ret, Some("there is an error".to_string()));
    }
}
