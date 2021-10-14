use nix::{errno::Errno, sys::socket};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::{error, fmt, io, marker::PhantomData, os::unix::prelude::RawFd};
use thiserror::Error;

#[derive(Error, Debug)]
pub enum WormholeError {
    #[error("sending on disconnected channel")]
    SendError,
    #[error("read error")]
    RecvError,
    #[error(transparent)]
    IOError(#[from] std::io::Error),
}

/// Implementation

#[derive(Debug)]
pub struct WormholeReceiver<T> {
    receiver: RawFd,
    phantom: PhantomData<T>,
}

pub struct WormholeSender<T> {
    sender: RawFd,
    phantom: PhantomData<T>,
}

impl<T> WormholeSender<T>
where
    T: Serialize,
{
    pub fn send(&self, object: T) -> Result<(), WormholeError> {
        // let payload = serde_json::to_vec(&object)?;

        Ok(())
    }
}

impl<T> WormholeReceiver<T> {
    fn recv(&self) -> Result<T, WormholeError> {
        todo!()
    }
}

/// Public APIs

pub fn channel<T>() -> Result<(WormholeSender<T>, WormholeReceiver<T>), io::Error>
where
    T: for<'de> Deserialize<'de> + Serialize,
{
    let (os_sender, os_receiver) = unix_channel()?;
    let receiver = WormholeReceiver {
        receiver: os_receiver,
        phantom: PhantomData,
    };
    let sender = WormholeSender {
        sender: os_sender,
        phantom: PhantomData,
    };
    Ok((sender, receiver))
}

// Use socketpair as the underlying pipe.
fn unix_channel() -> Result<(RawFd, RawFd), Errno> {
    socket::socketpair(
        socket::AddressFamily::Unix,
        socket::SockType::SeqPacket,
        None,
        socket::SockFlag::SOCK_CLOEXEC,
    )
}

#[cfg(test)]
mod tests {
    use crate::channel;

    #[test]
    fn it_works() {
        let (sender, receiver) = channel::<u32>().expect("failed to create channel");
    }
}
