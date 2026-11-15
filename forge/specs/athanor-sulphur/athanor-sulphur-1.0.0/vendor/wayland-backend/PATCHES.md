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

Changes 1 and 2 are not proposed to wayland-rs; whether and when is the maintainer's decision.

## When it goes

With the first release of `wayland-backend` whose parser rejects these nulls and whose server
tells a client its request was malformed: remove this
directory, its `[patch.crates-io]` entry and `exclude` entry in the root `Cargo.toml` and its
line in `VENDORED_CRATES` of `scripts/verify.py`, and update the lockfile with cargo.
