# wayland-backend 0.3.17, patched

The published crate `wayland-backend` 0.3.17 (crates.io, `MIT`, sha256
`38a91b4eaddff87b1cd1074985e3713da4af2c49742d1b356b2c01670a67a078`, wayland-rs commit
`72f7fe0d9e99cb720ed57ade0c3077aec533b6ab`, `wayland-backend`), with changes to its pure-Rust
backend. `[patch.crates-io]` in the repository's root `Cargo.toml` puts it in place of the
published crate for every crate of the workspace.

The wire parser of change 1 serves both sides of the pure-Rust backend. On the server it is
Sulphur's. On the client it is that of every workspace crate that reaches `wayland-client`
(the OSD, the launcher, the dock, the bar, the idle daemon, the control center and
`athanor-sulphur-client` among them): those clients now reject an event with a null in a slot
the signature does not admit, or with a new id of 0, with the protocol error "Malformed Wayland
message.", as libwayland's client does.
Only athanor-sal's optional `gpu` feature turns on `client_system`, which links libwayland's
client and does not use this parser.

## 1. A null the signature forbids is a malformed message

### Why

The wire parser read a string of length 0 as a null whatever the signature said, and took an
object or a new id of 0 as it came, so it accepted nulls the signature forbids. libwayland
rejects these nulls when it reads the message, and the patch brings the parser to the same
behaviour.

### The change

`src/rs/wire.rs`, `parse_message`: a null string where the signature says `AllowNull::No`, a
null object where it says `AllowNull::No` and a new id of 0 are `MessageParseError::Malformed`.

```diff
--- a/src/rs/wire.rs
+++ b/src/rs/wire.rs
@@ -4,7 +4,7 @@
 use std::ffi::CStr;
 use std::os::unix::io::{BorrowedFd, OwnedFd, RawFd};
 
-use crate::protocol::{Argument, ArgumentType, Message};
+use crate::protocol::{AllowNull, Argument, ArgumentType, Message};
 
 use smallvec::SmallVec;
 
@@ -204,7 +204,7 @@
                     ArgumentType::Int => Ok(Argument::Int(front as i32)),
                     ArgumentType::Uint => Ok(Argument::Uint(front)),
                     ArgumentType::Fixed => Ok(Argument::Fixed(front as i32)),
-                    ArgumentType::Str(_) => {
+                    ArgumentType::Str(allow_null) => {
                         read_array_from_payload(front as usize, tail).and_then(|(v, rest)| {
                             tail = rest;
                             if !v.is_empty() {
@@ -212,12 +212,19 @@
                                     Ok(s) => Ok(Argument::Str(Some(Box::new(s.into())))),
                                     Err(_) => Err(MessageParseError::Malformed),
                                 }
-                            } else {
+                            } else if allow_null == AllowNull::Yes {
                                 Ok(Argument::Str(None))
+                            } else {
+                                // A null where the signature forbids one, as libwayland rejects it.
+                                Err(MessageParseError::Malformed)
                             }
                         })
                     }
+                    ArgumentType::Object(AllowNull::No) if front == 0 => {
+                        Err(MessageParseError::Malformed)
+                    }
                     ArgumentType::Object(_) => Ok(Argument::Object(front)),
+                    ArgumentType::NewId if front == 0 => Err(MessageParseError::Malformed),
                     ArgumentType::NewId => Ok(Argument::NewId(front)),
                     ArgumentType::Array => {
                         read_array_from_payload(front as usize, tail).map(|(v, rest)| {
```

## 2. A malformed request ends its client with a protocol error

### Why

The server closed the connection of a client whose request it could not parse without telling
it why. libwayland posts `wl_display.error` with `invalid_method` first, and a client's log
then names the error instead of a lost connection.

### The change

`src/rs/server_impl/client.rs`, `Client::next_request`: a malformed request posts
`wl_display.error` (`invalid_method`), which ends the client as any protocol error does.

```diff
--- a/src/rs/server_impl/client.rs
+++ b/src/rs/server_impl/client.rs
@@ -369,7 +369,12 @@
                     continue;
                 }
                 Err(MessageParseError::Malformed) => {
-                    self.kill(DisconnectReason::ConnectionClosed);
+                    // The client learns why, as libwayland tells it: an unknown object or
+                    // opcode, or arguments its signature does not admit.
+                    self.post_display_error(
+                        DisplayError::InvalidMethod,
+                        CString::new("Malformed request.").unwrap(),
+                    );
                     return Err(rustix::io::Errno::PROTO.into());
                 }
             };
```

Sulphur's test `fixture::null_arguments::a_null_the_signature_forbids_ends_only_its_client`
fails without either change.

## 3. The workspace's `unsafe_code` denial stays outside the crate

### Why

Cargo caps the lints of a registry dependency, not those of a path dependency, so the
workspace's `-D unsafe_code` (`.cargo/config.toml`) reaches this copy and denies the unsafe
code every release of the crate carries.

### The change

`src/lib.rs` gains `#![allow(unsafe_code)]`, which restores the crate as published.

### Upstream

Changes 1, 2 and 4 are not proposed to wayland-rs; whether and when is the maintainer's decision.

## 4. One read per client per pass, one pass per dispatch

### Why

`dispatch_all_clients` polled its clients again until none had anything to read, and
`dispatch_events_for` read a client's socket until it would block. A client that writes requests
as fast as the compositor handles them never lets either loop end, so the compositor's event
loop never runs again: no other client is served, no frame is drawn, and the lock screen never
appears (wayland-rs#994). libwayland's server returns to its event loop after each client's
read.

### The change

`src/rs/server_impl/client.rs`, `Client::next_request`, takes `may_read` and reads from the
socket only while it is set, clearing it; past that it answers `WouldBlock` once the buffer
holds no whole request. `src/rs/server_impl/common_poll.rs`: `dispatch_events_for` allows one
read per call, and `dispatch_all_clients` makes one pass over the clients that epoll (or kqueue)
reports and returns. A pass handles at most one buffer, 4096 bytes, of each client's requests,
and leaves in the buffer at most part of a request, whose rest is still on the socket: the
socket stays readable, its level-triggered registration reports it again at the next pass, and
no whole request waits for a read that nothing would wake. A client still readable after its
pass is at the tail of epoll's ready list, so the clients are served in turn.

```diff
--- a/src/rs/server_impl/client.rs
+++ b/src/rs/server_impl/client.rs
@@ -346,6 +346,7 @@ impl<D> Client<D> {
     #[allow(clippy::type_complexity)]
     pub(crate) fn next_request(
         &mut self,
+        may_read: &mut bool,
     ) -> std::io::Result<(Message<u32, OwnedFd>, Object<Data<D>>)> {
         if self.killed {
             return Err(rustix::io::Errno::PIPE.into());
@@ -359,6 +360,11 @@ impl<D> Client<D> {
             }) {
                 Ok(msg) => msg,
                 Err(MessageParseError::MissingData) | Err(MessageParseError::MissingFD) => {
+                    // One read per pass: what the socket still holds waits for the next pass,
+                    // which its readiness brings, once the other clients have had theirs.
+                    if !std::mem::take(may_read) {
+                        return Err(rustix::io::Errno::AGAIN.into());
+                    }
                     // need to read more data
                     if let Err(e) = self.socket.fill_incoming_buffers() {
                         if e.kind() != std::io::ErrorKind::WouldBlock {
--- a/src/rs/server_impl/common_poll.rs
+++ b/src/rs/server_impl/common_poll.rs
@@ -84,24 +84,21 @@ impl<D> InnerBackend<D> {
         let poll_fd = self.poll_fd();
         let mut dispatched = 0;
         let mut events = Vec::<epoll::Event>::with_capacity(32);
-        loop {
-            let buffer = rustix::buffer::spare_capacity(&mut events);
-            epoll::wait(poll_fd.as_fd(), buffer, Some(&Timespec::default()))?;
-
-            if events.is_empty() {
-                break;
-            }
+        // One pass, then back to the event loop: the clients still readable wake it again, at
+        // the tail of the level-triggered ready list, so that one flooding client neither keeps
+        // the loop here nor goes ahead of the others.
+        let buffer = rustix::buffer::spare_capacity(&mut events);
+        epoll::wait(poll_fd.as_fd(), buffer, Some(&Timespec::default()))?;
 
-            for event in events.drain(..) {
-                let id = InnerClientId::from_u64(event.data.u64());
-                // remove the cb while we call it, to gracefully handle reentrancy
-                if let Ok(count) = self.dispatch_events_for(data, id) {
-                    dispatched += count;
-                }
+        for event in events.drain(..) {
+            let id = InnerClientId::from_u64(event.data.u64());
+            // remove the cb while we call it, to gracefully handle reentrancy
+            if let Ok(count) = self.dispatch_events_for(data, id) {
+                dispatched += count;
             }
-            let cleanup = self.state.lock().unwrap().cleanup();
-            cleanup(&self.handle(), data);
         }
+        let cleanup = self.state.lock().unwrap().cleanup();
+        cleanup(&self.handle(), data);
 
         Ok(dispatched)
     }
@@ -119,24 +116,19 @@ impl<D> InnerBackend<D> {
         let poll_fd = self.poll_fd();
         let mut dispatched = 0;
         let mut events = Vec::<Event>::with_capacity(32);
-        loop {
-            let buffer = rustix::buffer::spare_capacity(&mut events);
-            let nevents = unsafe { kevent(&poll_fd, &[], buffer, Some(Duration::ZERO))? };
-
-            if nevents == 0 {
-                break;
-            }
+        // One pass, as on Linux above.
+        let buffer = rustix::buffer::spare_capacity(&mut events);
+        unsafe { kevent(&poll_fd, &[], buffer, Some(Duration::ZERO))? };
 
-            for event in events.drain(..) {
-                let id = InnerClientId::from_u64(event.udata() as u64);
-                // remove the cb while we call it, to gracefully handle reentrancy
-                if let Ok(count) = self.dispatch_events_for(data, id) {
-                    dispatched += count;
-                }
+        for event in events.drain(..) {
+            let id = InnerClientId::from_u64(event.udata() as u64);
+            // remove the cb while we call it, to gracefully handle reentrancy
+            if let Ok(count) = self.dispatch_events_for(data, id) {
+                dispatched += count;
             }
-            let cleanup = self.state.lock().unwrap().cleanup();
-            cleanup(&self.handle(), data);
         }
+        let cleanup = self.state.lock().unwrap().cleanup();
+        cleanup(&self.handle(), data);
 
         Ok(dispatched)
     }
@@ -147,13 +139,15 @@ impl<D> InnerBackend<D> {
         client_id: InnerClientId,
     ) -> std::io::Result<usize> {
         let mut dispatched = 0;
+        // The one read from the client's socket this pass allows.
+        let mut may_read = true;
         let handle = self.handle();
         let mut state = self.state.lock().unwrap();
         loop {
             let action = {
                 let state = &mut *state;
                 if let Ok(client) = state.clients.get_client_mut(client_id.clone()) {
-                    let (message, object) = match client.next_request() {
+                    let (message, object) = match client.next_request(&mut may_read) {
                         Ok(v) => v,
                         Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                             if dispatched > 0 {
```

Sulphur's tests `fixture::flood::a_flooding_client_does_not_starve_the_others`, which fails
without the change, and `fixture::flood::a_burst_longer_than_one_read_is_served_to_its_end`,
which would fail if a pass left whole requests in the buffer.

## When it goes

With the first release of `wayland-backend` whose parser rejects these nulls, whose server
tells a client its request was malformed and returns to its event loop after one read of each
client: remove this
directory, its `[patch.crates-io]` entry and `exclude` entry in the root `Cargo.toml` and its
line in `VENDORED_CRATES` of `scripts/verify.py`, and update the lockfile with cargo.
