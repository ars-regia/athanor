//! Drawing on vello_gpu: the scene of `render.rs`, rasterised through wgpu straight onto the
//! window's wl_surface (doc_own_stack.md section 7, the renderer milestone T1 measures).
//!
//! wgpu takes the window as libwayland-client's raw pointers, which no safe interface hands
//! over: this is the only module of the crate with `unsafe` code.

#![allow(unsafe_code)]

use crate::render::{layout, paint, Error};
use crate::widget::{Kind, Ui};
use std::future::Future;
use std::pin::pin;
use std::ptr::NonNull;
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll, Waker};
use vello_gpu::{
    ClearSettings, ImageId, ImageSource, Pixmap, RenderSize, RenderTargetConfig, TargetInit,
    TextureBindings,
};
use wayland_client::{protocol::wl_surface::WlSurface, Connection, Proxy};
use wgpu::rwh::{HasDisplayHandle, RawWindowHandle, WaylandWindowHandle};
use wgpu::{CurrentSurfaceTexture, PresentMode, TextureFormat};

/// The GPU renderer of one Wayland surface, drawing with vello_gpu through wgpu.
pub struct Gpu {
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
    renderer: vello_gpu::Renderer,
    resources: Resources,
    scene: vello_gpu::Scene,
    /// The first error wgpu reported outside a call's result, which the next frame returns.
    errors: Arc<Mutex<Option<String>>>,
    /// The adapter wgpu picked, as the start-up line names it.
    pub adapter: String,
}

/// The total area of the images uploaded to vello_gpu's atlas, in pixels: one atlas of the eight
/// it may grow to. vello_gpu panics when an upload finds no room (`upload_image` unwraps the
/// allocation), and an image is at most `MAX_IMAGE_SIDE` on a side, so the room this leaves
/// keeps the packing from failing.
// ponytail: an area budget, not the atlas's own accounting; read the allocator's free space
// if vello_gpu ever exposes it.
const ATLAS_BUDGET: u64 = 4096 * 4096;

/// vello_gpu's resources, and the images of the window uploaded to its atlas. vello_gpu 0.3
/// panics on an image paint that carries its pixmap ("pixmap image sources are not supported
/// by Vello GPU"): an image must be uploaded first and drawn by the id the upload returns.
pub struct Resources {
    vello: vello_gpu::Resources,
    /// Each uploaded pixmap with its id. The `Arc` is held, so that the address it is matched
    /// by is not reused while it is listed.
    images: Vec<(Arc<Pixmap>, ImageId)>,
}

impl Resources {
    /// The uploaded copy of `pixmap`, if there is one.
    pub(crate) fn image(&self, pixmap: &Arc<Pixmap>) -> Option<ImageSource> {
        self.images
            .iter()
            .find(|(uploaded, _)| Arc::ptr_eq(uploaded, pixmap))
            .map(|(_, id)| ImageSource::opaque_id(*id))
    }

    /// What vello_gpu draws glyphs with.
    pub(crate) fn glyphs(&mut self) -> &mut vello_gpu::Resources {
        &mut self.vello
    }
}

/// The output of a future wgpu resolves at once, as its native backends do for adapters and
/// devices.
fn ready<T>(future: impl Future<Output = T>) -> Result<T, Error> {
    match pin!(future).poll(&mut Context::from_waker(Waker::noop())) {
        Poll::Ready(output) => Ok(output),
        Poll::Pending => Err("wgpu left a request pending".into()),
    }
}

impl Gpu {
    /// Opens a wgpu device that presents to `surface`. With no adapter that can, it fails: the
    /// caller asked for the GPU, and drawing on the CPU instead would hide it.
    ///
    /// # Safety
    ///
    /// `conn` and `surface` must outlive the returned value.
    pub unsafe fn new(conn: &Connection, surface: &WlSurface) -> Result<Self, Error> {
        let backend = conn.backend();
        let display = backend.display_handle()?.as_raw();
        let window = NonNull::new(surface.id().as_ptr().cast())
            .ok_or("the window's wl_surface is already destroyed")?;
        // WGPU_BACKEND and the other variables of wgpu's own pick the backend.
        let instance = wgpu::Instance::new(
            wgpu::InstanceDescriptor::new_with_display_handle_from_env(Box::new(backend)),
        );
        // SAFETY: the instance keeps a clone of the connection's backend, which keeps the
        // display open; the wl_surface outlives the wgpu surface by the caller's promise.
        let surface = unsafe {
            instance.create_surface_unsafe(wgpu::SurfaceTargetUnsafe::RawHandle {
                raw_display_handle: Some(display),
                raw_window_handle: RawWindowHandle::Wayland(WaylandWindowHandle::new(window)),
            })
        }?;
        let adapter = ready(instance.request_adapter(&wgpu::RequestAdapterOptions {
            compatible_surface: Some(&surface),
            ..Default::default()
        }))?
        .map_err(|e| format!("no GPU adapter can draw on the window: {e}"))?;
        let info = adapter.get_info();
        let adapter_name = format!(
            "{} ({:?}, {:?}, {} {})",
            info.name, info.backend, info.device_type, info.driver, info.driver_info
        );
        // vello_gpu needs no more than WebGL 2 offers; ask for what the adapter has.
        let (device, queue) = ready(adapter.request_device(&wgpu::DeviceDescriptor {
            required_limits: adapter.limits(),
            ..Default::default()
        }))??;
        let errors = Arc::new(Mutex::new(None));
        let slot = Arc::clone(&errors);
        device.on_uncaptured_error(Arc::new(move |error| {
            if let Ok(mut slot) = slot.lock() {
                slot.get_or_insert_with(|| error.to_string());
            }
        }));
        let capabilities = surface.get_capabilities(&adapter);
        // vello_gpu, as vello_cpu, writes sRGB-encoded values: the surface must not encode them
        // again.
        let format = capabilities
            .formats
            .into_iter()
            .find(|f| matches!(f, TextureFormat::Rgba8Unorm | TextureFormat::Bgra8Unorm))
            .ok_or_else(|| format!("{adapter_name} offers no 8-bit linear surface format"))?;
        // Mailbox where Vulkan offers it, otherwise Fifo, the one mode wgpu offers on GLES. The
        // driver draws only after the compositor's frame callback for the last frame, which
        // arrives with the one Mesa's Fifo waits for, so that wait stays short.
        let present_mode = if capabilities.present_modes.contains(&PresentMode::Mailbox) {
            PresentMode::Mailbox
        } else {
            PresentMode::Fifo
        };
        let config = wgpu::SurfaceConfiguration {
            format,
            present_mode,
            // Not configured yet: the first frame configures the window's size.
            width: 0,
            height: 0,
            ..surface
                .get_default_config(&adapter, 1, 1)
                .ok_or_else(|| format!("{adapter_name} cannot present to the window"))?
        };
        let (renderer, resources) = vello_gpu::Renderer::new(
            &device,
            &RenderTargetConfig {
                format,
                width: 1,
                height: 1,
            },
        );
        Ok(Self {
            surface,
            device,
            queue,
            config,
            renderer,
            resources: Resources {
                vello: resources,
                images: Vec::new(),
            },
            scene: vello_gpu::Scene::new(1, 1),
            errors,
            adapter: adapter_name,
        })
    }

    /// Lays the window out, draws it at `scale` and presents it, which commits the surface.
    /// Returns the size drawn, in physical pixels.
    pub fn draw(&mut self, ui: &mut Ui, scale: f32) -> Result<(u16, u16), Error> {
        let (width, height) = layout(ui, scale)?;
        let size = (u32::from(width), u32::from(height));
        if (self.config.width, self.config.height) != size {
            (self.config.width, self.config.height) = size;
            self.surface.configure(&self.device, &self.config);
        }
        self.scene.reset_and_resize(width, height);
        let mut encoder = self.device.create_command_encoder(&Default::default());
        self.upload_images(ui, &mut encoder)?;
        paint(ui, scale, &mut self.scene, &mut self.resources)?;
        let frame = match self.surface.get_current_texture() {
            CurrentSurfaceTexture::Success(frame) | CurrentSurfaceTexture::Suboptimal(frame) => {
                frame
            }
            other => return Err(format!("the window's surface has no frame: {other:?}").into()),
        };
        let view = frame.texture.create_view(&Default::default());
        self.renderer.render(
            &self.scene,
            &mut self.resources.vello,
            &self.device,
            &self.queue,
            &mut encoder,
            &RenderSize { width, height },
            &view,
            None,
            &TextureBindings::new(),
            TargetInit::Clear(ClearSettings::default()),
        )?;
        self.queue.submit([encoder.finish()]);
        self.check()?;
        self.queue.present(frame);
        // A present that fails reaches the error slot; one that ends Outdated, Lost, Timeout or
        // Occluded does not (wgpu 30 drops that status), and may commit nothing: the driver
        // commits the surface after this, so its frame callback comes in either case.
        self.check()?;
        Ok((width, height))
    }

    /// Uploads the images of `ui`'s widgets that the atlas lacks, and frees those no widget
    /// holds any more. Fails, uploading nothing more, when the images would pass
    /// [`ATLAS_BUDGET`].
    fn upload_images(&mut self, ui: &Ui, encoder: &mut wgpu::CommandEncoder) -> Result<(), Error> {
        let wanted: Vec<&Arc<Pixmap>> = (0..ui.len())
            .filter_map(|id| match &ui.widget(id).kind {
                Kind::Image { image, .. } => Some(image.pixmap()),
                _ => None,
            })
            .collect();
        let resources = &mut self.resources;
        let (kept, gone): (Vec<_>, Vec<_>) = std::mem::take(&mut resources.images)
            .into_iter()
            .partition(|(uploaded, _)| wanted.iter().any(|w| Arc::ptr_eq(w, uploaded)));
        for (_, id) in gone {
            self.renderer
                .destroy_image(&mut resources.vello, encoder, id);
        }
        resources.images = kept;
        let area = |pixmap: &Pixmap| u64::from(pixmap.width()) * u64::from(pixmap.height());
        let mut used: u64 = resources.images.iter().map(|(p, _)| area(p)).sum();
        for pixmap in wanted {
            if resources.image(pixmap).is_some() {
                continue;
            }
            used += area(pixmap);
            if used > ATLAS_BUDGET {
                return Err(format!(
                    "the window's images pass the GPU atlas budget of {ATLAS_BUDGET} pixels"
                )
                .into());
            }
            let id = self.renderer.upload_image(
                &mut resources.vello,
                &self.device,
                &self.queue,
                encoder,
                pixmap,
            );
            resources.images.push((Arc::clone(pixmap), id));
        }
        Ok(())
    }

    /// Fails with the first error wgpu reported since the last check.
    fn check(&self) -> Result<(), Error> {
        let error = self
            .errors
            .lock()
            .map_err(|_| "wgpu's error slot is poisoned")?
            .take();
        match error {
            Some(error) => Err(format!("wgpu: {error}").into()),
            None => Ok(()),
        }
    }
}
