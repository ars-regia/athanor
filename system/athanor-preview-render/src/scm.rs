//! The one receive with `SCM_RIGHTS` the wallpaper's decoder makes: the request and the file
//! that rides on it. GIO's control-message list reads a null pointer as a slice when no
//! message came, so the call is nix's, and the descriptors it hands over as raw numbers are
//! owned here.

#![allow(unsafe_code)]

use std::io::{self, IoSliceMut};
use std::os::fd::{AsRawFd, BorrowedFd, FromRawFd, OwnedFd, RawFd};

use nix::sys::socket::{recvmsg, ControlMessageOwned, MsgFlags};

/// Reads into `buf` from `socket` once, with up to two descriptors: two, so that a request
/// with more than one file is seen and refused, never cut to its first. Every descriptor
/// received is owned and closed on drop; a truncated control message is an error.
pub fn receive(socket: BorrowedFd<'_>, buf: &mut [u8]) -> io::Result<(usize, Vec<OwnedFd>)> {
    let mut space = nix::cmsg_space!([RawFd; 2]);
    let mut iov = [IoSliceMut::new(buf)];
    let msg = recvmsg::<()>(socket.as_raw_fd(), &mut iov, Some(&mut space), MsgFlags::MSG_CMSG_CLOEXEC)?;
    let mut fds = Vec::new();
    for message in msg.cmsgs()? {
        if let ControlMessageOwned::ScmRights(raw) = message {
            for fd in raw {
                // SAFETY: the kernel installed each descriptor of SCM_RIGHTS in this process
                // for this message; nothing else holds it.
                fds.push(unsafe { OwnedFd::from_raw_fd(fd) });
            }
        }
    }
    if msg.flags.contains(MsgFlags::MSG_CTRUNC) {
        return Err(io::Error::other("more descriptors than a request carries"));
    }
    Ok((msg.bytes, fds))
}
