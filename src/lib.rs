mod error;

use error::Result;
use nix::sys::{socket, uio};
use serde::{Deserialize, Serialize};
use std::{marker::PhantomData, os::unix::prelude::RawFd};

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
    pub fn send(&self, object: T) -> Result<()> {
        let payload = serde_json::to_vec(&object)?;

        Ok(())
    }

    pub fn send_iovec(
        &mut self,
        iov: &[uio::IoVec<&[u8]>],
        fds: Option<&[RawFd]>,
    ) -> Result<usize> {
        if let Some(rfds) = fds {
            socket::sendmsg(
                self.sender,
                iov,
                &[socket::ControlMessage::ScmRights(rfds)],
                socket::MsgFlags::empty(),
                None,
            )
            .map_err(|e| e.into())
        } else {
            socket::sendmsg(self.sender, iov, &[], socket::MsgFlags::empty(), None)
                .map_err(|e| e.into())
        }
    }

    pub fn send_slice(&mut self, data: &[u8], fds: Option<&[RawFd]>) -> Result<usize> {
        let iov = [uio::IoVec::from_slice(data)];
        self.send_iovec(&iov[..], fds)
    }
}

impl<T> WormholeReceiver<T> {
    fn recv(&self) -> Result<T> {
        todo!()
    }
}

pub fn channel<T>() -> Result<(WormholeSender<T>, WormholeReceiver<T>)>
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
fn unix_channel() -> Result<(RawFd, RawFd)> {
    Ok(socket::socketpair(
        socket::AddressFamily::Unix,
        socket::SockType::SeqPacket,
        None,
        socket::SockFlag::SOCK_CLOEXEC,
    )?)
}

#[cfg(test)]
mod tests {
    use crate::channel;

    #[test]
    fn it_works() {
        let (sender, receiver) = channel::<u32>().expect("failed to create channel");
    }
}
