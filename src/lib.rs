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
    fn send_iovec(&mut self, iov: &[uio::IoVec<&[u8]>], fds: Option<&[RawFd]>) -> Result<usize> {
        let cmsgs = if let Some(fds) = fds {
            vec![socket::ControlMessage::ScmRights(fds)]
        } else {
            vec![]
        };
        socket::sendmsg(self.sender, iov, &cmsgs, socket::MsgFlags::empty(), None)
            .map_err(|e| e.into())
    }

    fn send_slice_with_len(&mut self, data: &[u8], fds: Option<&[RawFd]>) -> Result<usize> {
        let len = data.len() as u64;
        // Here we prefix the length of the data onto the serialized data.
        let iov = [
            uio::IoVec::from_slice(unsafe {
                std::slice::from_raw_parts(
                    (&len as *const u64) as *const u8,
                    std::mem::size_of::<u64>(),
                )
            }),
            uio::IoVec::from_slice(data),
        ];
        self.send_iovec(&iov[..], fds)
    }

    pub fn send(&mut self, object: T) -> Result<()> {
        let payload = serde_json::to_vec(&object)?;
        self.send_slice_with_len(&payload, None)?;

        Ok(())
    }

    pub fn send_fds(&mut self, object: T, fds: &[RawFd]) -> Result<()> {
        let payload = serde_json::to_vec(&object)?;
        self.send_slice_with_len(&payload, Some(fds))?;

        Ok(())
    }
}

impl<T> WormholeReceiver<T>
where
    T: serde::de::DeserializeOwned,
{
    fn peek_size_iovec(&mut self) -> Result<u64> {
        let mut len: u64 = 0;
        let iov = [uio::IoVec::from_mut_slice(unsafe {
            std::slice::from_raw_parts_mut(
                (&mut len as *mut u64) as *mut u8,
                std::mem::size_of::<u64>(),
            )
        })];
        let _ = socket::recvmsg(self.receiver, &iov, None, socket::MsgFlags::MSG_PEEK)?;

        Ok(len)
    }

    fn recv_into_iovec<F>(&mut self, iov: &[uio::IoVec<&mut [u8]>]) -> Result<(usize, Option<F>)>
    where
        F: Default + AsMut<[RawFd]>,
    {
        let mut cmsgspace = nix::cmsg_space!(F);
        let msg = socket::recvmsg(
            self.receiver,
            iov,
            Some(&mut cmsgspace),
            socket::MsgFlags::MSG_CMSG_CLOEXEC,
        )?;

        // Sending multiple SCM_RIGHTS message will led to platform dependent
        // behavior, with some system choose to return EINVAL when sending or
        // silently only process the first msg or send all of it. Here we assume
        // there is only one SCM_RIGHTS message and will only process the first
        // message.
        let fds: Option<F> = msg
            .cmsgs()
            .find_map(|cmsg| {
                if let socket::ControlMessageOwned::ScmRights(fds) = cmsg {
                    Some(fds)
                } else {
                    None
                }
            })
            .map(|fds| {
                let mut fds_array: F = Default::default();
                <F as AsMut<[RawFd]>>::as_mut(&mut fds_array).clone_from_slice(&fds);

                fds_array
            });

        Ok((msg.bytes, fds))
    }

    fn recv_into_buf_with_len<F>(&mut self) -> Result<(Vec<u8>, Option<F>)>
    where
        F: Default + AsMut<[RawFd]>,
    {
        let msg_len = self.peek_size_iovec()?;
        let mut len: u64 = 0;
        let mut buf = vec![0u8; msg_len as usize];
        let (_, fds) = {
            let iov = [
                uio::IoVec::from_mut_slice(unsafe {
                    std::slice::from_raw_parts_mut(
                        (&mut len as *mut u64) as *mut u8,
                        std::mem::size_of::<u64>(),
                    )
                }),
                uio::IoVec::from_mut_slice(&mut buf),
            ];
            self.recv_into_iovec(&iov)?
        };
        Ok((buf, fds))
    }

    // Recv the next message of type T.
    pub fn recv(&mut self) -> Result<T> {
        let (buf, _) = self.recv_into_buf_with_len::<[RawFd; 0]>()?;
        Ok(serde_json::from_slice(&buf[..])?)
    }

    // Works similar to `recv`, but will look for fds sent by SCM_RIGHTS
    // message.  We use F as as `[RawFd; n]`, where `n` is the number of
    // descriptors you want to receive.
    pub fn recv_with_fds<F>(&mut self) -> Result<(T, Option<F>)>
    where
        F: Default + AsMut<[RawFd]>,
    {
        let (buf, fds) = self.recv_into_buf_with_len::<F>()?;
        Ok((serde_json::from_slice(&buf[..])?, fds))
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
    use nix::sys::wait;
    use serde::{Deserialize, Serialize};

    #[derive(Debug, Default, Serialize, Deserialize, PartialEq)]
    struct TestMessage {
        a: i32,
        b: String,
        c: Vec<i32>,
    }

    #[test]
    fn basic() {
        let (mut sender, mut receiver) =
            channel::<TestMessage>().expect("failed to create channel");
        let test_message = TestMessage {
            a: 1776,
            b: String::from("hello world!"),
            c: vec![5, 4, 3, 2, 1],
        };

        match unsafe { nix::unistd::fork().expect("failed fork") } {
            nix::unistd::ForkResult::Parent { child } => {
                let res = receiver
                    .recv()
                    .expect("failed to receive from child process");
                wait::waitpid(child, None).expect("failed wait for pid");
                assert_eq!(
                    test_message, res,
                    "received message doesn't match the send message"
                );
            }
            nix::unistd::ForkResult::Child => {
                sender
                    .send(test_message)
                    .expect("failed to send from the child process");
                std::process::exit(0);
            }
        };
    }
}
