//! wl_drm (doc_own_stack.md decision 33, wave 2b, `standard`), for the older Mesa clients, and
//! the Xwayland and Wine paths, that look for it before linux-dmabuf. It is written from Mesa's
//! description, `protocols/wayland-drm.xml` (README.md names its commit and its licence), and
//! from no compositor's code (OS3).
//!
//! - It is offered on a render node only, the one linux-dmabuf's feedback names: `device` sends
//!   that node's path.
//! - `format` names each format the renderer imports with the implicit modifier, the only one
//!   a buffer of this protocol can carry; `capabilities` names prime.
//! - A buffer comes only from `create_prime_buffer`: its parameters are checked as linux-dmabuf
//!   checks its own, and the renderer imports it as a dmabuf of linux-dmabuf. `create_buffer`
//!   and `create_planar_buffer` name a GEM flink name, which a render node does not have, and
//!   are the `invalid_name` error.
//! - `authenticate` is answered with `authenticated`, and authenticates nothing: the node named
//!   is a render node, which needs no DRM authentication, and the compositor calls no
//!   `drmAuthMagic`, so a client that asks it for a primary node it opened gains no access.

use crate::state::State;
use protocol::wl_drm::{self, WlDrm};
use smithay::backend::allocator::dmabuf::{Dmabuf, DmabufFlags};
use smithay::backend::allocator::format::FormatSet;
use smithay::backend::allocator::{Fourcc, Modifier};
use smithay::backend::drm::DrmNode;
use smithay::reexports::wayland_server::{Client, DataInit, DisplayHandle, New, Resource};
use smithay::wayland::{Dispatch2, GlobalDispatch2};
use std::fs::File;
use std::io::{Seek, SeekFrom};
use std::os::fd::OwnedFd;
use std::sync::Arc;

/// The server side of Mesa's description, laid out as `wayland-protocols` lays out its own.
#[allow(
    dead_code,
    missing_docs,
    non_camel_case_types,
    non_snake_case,
    non_upper_case_globals,
    unused_imports,
    unused_variables,
    clippy::all
)]
pub mod protocol {
    use smithay::reexports::wayland_server;
    use smithay::reexports::wayland_server::protocol::*;

    pub mod __interfaces {
        // The generated tables name `wayland_backend`, which wayland-server re-exports.
        use smithay::reexports::wayland_server::backend as wayland_backend;
        use smithay::reexports::wayland_server::protocol::__interfaces::*;
        wayland_scanner::generate_interfaces!("protocols/wayland-drm.xml");
    }
    use self::__interfaces::*;

    wayland_scanner::generate_server_code!("protocols/wayland-drm.xml");
}

/// The data of the global and of every wl_drm object: the render node's path and the codes of
/// the formats offered, sorted.
#[derive(Clone)]
pub struct Drm {
    device: Arc<str>,
    formats: Arc<[u32]>,
}

/// Offers the global for the render node `node`, whose renderer imports `formats`. A node
/// without a path in UTF-8, which a device under /dev/dri never is, is not named: the global is
/// then not offered, and a warning says so.
pub fn global(dh: &DisplayHandle, node: DrmNode, formats: &FormatSet) {
    let Some(device) = node
        .dev_path()
        .and_then(|path| path.into_os_string().into_string().ok())
    else {
        tracing::warn!(%node, "wl_drm is not offered: the render node has no path to name");
        return;
    };
    let mut codes: Vec<u32> = formats
        .iter()
        .filter(|format| format.modifier == Modifier::Invalid)
        .map(|format| format.code as u32)
        .collect();
    codes.sort_unstable();
    codes.dedup();
    let drm = Drm {
        device: device.into(),
        formats: codes.into(),
    };
    dh.create_global::<State, WlDrm, _>(2, drm);
}

impl GlobalDispatch2<WlDrm, State> for Drm {
    fn bind(
        &self,
        _state: &mut State,
        _dh: &DisplayHandle,
        _client: &Client,
        resource: New<WlDrm>,
        data_init: &mut DataInit<'_, State>,
    ) {
        let drm = data_init.init(resource, self.clone());
        drm.device(self.device.to_string());
        for format in self.formats.iter() {
            drm.format(*format);
        }
        if drm.version() >= 2 {
            drm.capabilities(wl_drm::Capability::Prime.into());
        }
    }
}

impl Dispatch2<WlDrm, State> for Drm {
    fn request(
        &self,
        state: &mut State,
        _client: &Client,
        drm: &WlDrm,
        request: wl_drm::Request,
        _dh: &DisplayHandle,
        data_init: &mut DataInit<'_, State>,
    ) {
        use wl_drm::Request;
        match request {
            Request::Authenticate { .. } => drm.authenticated(),
            // The error kills the client, so the new buffer needs no data.
            Request::CreateBuffer { .. } | Request::CreatePlanarBuffer { .. } => drm.post_error(
                wl_drm::Error::InvalidName,
                "a render node has no flink names: use create_prime_buffer",
            ),
            Request::CreatePrimeBuffer {
                id,
                name,
                width,
                height,
                format,
                offset0,
                stride0,
                offset1,
                stride1,
                offset2,
                stride2,
            } => {
                let planes = [(offset0, stride0), (offset1, stride1), (offset2, stride2)];
                let dmabuf = match self.prime(name, (width, height), format, planes) {
                    Ok(dmabuf) => dmabuf,
                    Err((error, message)) => return drm.post_error(error, message),
                };
                if let Err(error) = state.import_dmabuf(&dmabuf) {
                    tracing::warn!(%error, "a wl_drm buffer could not be imported");
                    return drm.post_error(
                        wl_drm::Error::InvalidName,
                        "the renderer cannot import the buffer",
                    );
                }
                // Smithay's dmabuf buffer: the renderer finds it as a linux-dmabuf one.
                data_init.init(id, dmabuf);
            }
        }
    }
}

impl Drm {
    /// The dmabuf of a `create_prime_buffer` on `fd`: a plane for the first pair of `planes`,
    /// and for each other whose stride is not 0, as the description passes 0 and 0 for an
    /// unused plane, all on `fd`, with the implicit modifier. The checks are linux-dmabuf's: a
    /// format offered, a positive size, and when `fd`'s size is known, each plane inside it.
    fn prime(
        &self,
        fd: OwnedFd,
        (width, height): (i32, i32),
        format: u32,
        planes: [(i32, i32); 3],
    ) -> Result<Dmabuf, (wl_drm::Error, String)> {
        let invalid = |message: String| (wl_drm::Error::InvalidName, message);
        let fourcc = Fourcc::try_from(format)
            .ok()
            .filter(|_| self.formats.binary_search(&format).is_ok())
            .ok_or_else(|| {
                (
                    wl_drm::Error::InvalidFormat,
                    format!("the format {format:#010x} is not offered"),
                )
            })?;
        let rows = u32::try_from(height)
            .ok()
            .filter(|_| width > 0 && height > 0)
            .ok_or_else(|| invalid(format!("the size {width}x{height} is not positive")))?;
        // As linux-dmabuf, a descriptor that cannot seek is left to the renderer's import.
        let mut file = File::from(fd);
        let size = file.seek(SeekFrom::End(0)).ok();
        let fd = Arc::new(OwnedFd::from(file));

        let mut builder = Dmabuf::builder(
            (width, height),
            fourcc,
            Modifier::Invalid,
            DmabufFlags::empty(),
        );
        for (index, (offset, stride)) in planes.into_iter().enumerate() {
            if index > 0 && stride == 0 {
                continue;
            }
            let (Ok(offset), Ok(stride)) = (u32::try_from(offset), u32::try_from(stride)) else {
                return Err(invalid(format!(
                    "plane {index}: the offset {offset} or the stride {stride} is negative"
                )));
            };
            let start = u64::from(offset);
            let row = start + u64::from(stride);
            let end = start + u64::from(stride) * u64::from(rows);
            // A plane after the first may be subsampled, so only the first is held to its rows.
            if let Some(size) = size {
                if start > size || row > size || (index == 0 && end > size) {
                    return Err(invalid(format!(
                        "plane {index}: the offset {offset} and the stride {stride} reach past \
                         the buffer's {size} bytes"
                    )));
                }
            }
            builder.add_plane(fd.clone(), offset, stride);
        }
        builder
            .build()
            .ok_or_else(|| invalid("the buffer has no plane".to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use smithay::backend::allocator::Buffer;

    const ARGB: u32 = Fourcc::Argb8888 as u32;

    fn drm() -> Drm {
        Drm {
            device: "/dev/dri/renderD128".into(),
            // Sorted, as `global` sorts them: NV12's code is the lower.
            formats: vec![Fourcc::Nv12 as u32, ARGB].into(),
        }
    }

    /// A file of `len` bytes, already unlinked; `tag` keeps the tests' names apart.
    fn file(len: u64, tag: &str) -> OwnedFd {
        let path = std::env::temp_dir().join(format!(
            "athanor-sulphur-wl-drm-{}-{tag}",
            std::process::id()
        ));
        let file = File::options()
            .read(true)
            .write(true)
            .create_new(true)
            .open(&path)
            .expect("a temporary file");
        std::fs::remove_file(&path).expect("unlinked");
        file.set_len(len).expect("its size");
        file.into()
    }

    fn error(result: Result<Dmabuf, (wl_drm::Error, String)>) -> wl_drm::Error {
        match result {
            Ok(_) => panic!("the buffer was accepted"),
            Err((error, _)) => error,
        }
    }

    #[test]
    fn a_prime_buffer_is_a_dmabuf_with_the_implicit_modifier() {
        let planes = [(0, 256), (0, 0), (0, 0)];
        let dmabuf = drm()
            .prime(file(256 * 64, "one"), (64, 64), ARGB, planes)
            .expect("accepted");
        assert_eq!(dmabuf.format().code, Fourcc::Argb8888);
        assert_eq!(dmabuf.format().modifier, Modifier::Invalid);
        assert_eq!((dmabuf.size().w, dmabuf.size().h), (64, 64));
        assert_eq!(dmabuf.num_planes(), 1, "an unused plane is 0 and 0");
        assert_eq!(dmabuf.strides().collect::<Vec<_>>(), [256]);
    }

    #[test]
    fn each_plane_with_a_stride_is_a_plane_of_the_same_fd() {
        // NV12, 64x64: the chroma plane is half as high, after the luma plane.
        let planes = [(0, 64), (64 * 64, 64), (0, 0)];
        let dmabuf = drm()
            .prime(file(64 * 96, "two"), (64, 64), Fourcc::Nv12 as u32, planes)
            .expect("accepted");
        assert_eq!(dmabuf.num_planes(), 2);
        assert_eq!(dmabuf.offsets().collect::<Vec<_>>(), [0, 64 * 64]);
    }

    #[test]
    fn a_format_not_offered_is_invalid_format() {
        let planes = [(0, 256), (0, 0), (0, 0)];
        let xrgb = Fourcc::Xrgb8888 as u32;
        let result = drm().prime(file(256 * 64, "format"), (64, 64), xrgb, planes);
        assert_eq!(error(result), wl_drm::Error::InvalidFormat);
        let result = drm().prime(file(256 * 64, "code"), (64, 64), 0x1234, planes);
        assert_eq!(error(result), wl_drm::Error::InvalidFormat, "not a fourcc");
    }

    #[test]
    fn a_size_or_a_plane_out_of_bounds_is_invalid_name() {
        let drm = drm();
        let one = |offset, stride| [(offset, stride), (0, 0), (0, 0)];
        for (size, planes, what) in [
            ((0, 64), one(0, 256), "no width"),
            ((64, -1), one(0, 256), "a negative height"),
            ((64, 64), one(-4, 256), "a negative offset"),
            ((64, 64), one(0, -256), "a negative stride"),
            ((64, 64), one(0, 257), "rows past the end"),
            ((64, 64), one(256 * 64 + 1, 0), "an offset past the end"),
            (
                (64, 64),
                [(0, 256), (0, -1), (0, 0)],
                "a negative second stride",
            ),
            (
                (64, 64),
                [(0, 256), (256 * 64, 1), (0, 0)],
                "a second row past the end",
            ),
        ] {
            let result = drm.prime(file(256 * 64, what), size, ARGB, planes);
            assert_eq!(error(result), wl_drm::Error::InvalidName, "{what}");
        }
    }
}
