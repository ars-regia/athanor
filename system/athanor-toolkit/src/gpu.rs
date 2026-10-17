//! Drawing on vello_gpu: the scene of `render.rs`, rasterised through wgpu straight onto the
//! window's wl_surface (doc_own_stack.md section 7, the renderer milestone T1 measures).
//!
//! wgpu takes the window as libwayland-client's raw pointers, which no safe interface hands
//! over: this is the only module of the crate with `unsafe` code.

#![allow(unsafe_code)]

use crate::render::{layout, paint, Error};
use crate::widget::Ui;
use std::future::Future;
use std::pin::pin;
use std::ptr::NonNull;
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll, Waker};
use vello_gpu::{ClearSettings, RenderSize, RenderTargetConfig, TargetInit, TextureBindings};
use wayland_client::{protocol::wl_surface::WlSurface, Connection, Proxy};
use wgpu::rwh::{HasDisplayHandle, RawWindowHandle, WaylandWindowHandle};
use wgpu::{CurrentSurfaceTexture, PresentMode, TextureFormat};

pub struct Gpu {
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
    renderer: vello_gpu::Renderer,
    resources: vello_gpu::Resources,
    scene: vello_gpu::Scene,
    /// The first error wgpu reported outside a call's result, which the next frame returns.
    errors: Arc<Mutex<Option<String>>>,
    /// The adapter wgpu picked, as the start-up line names it.
    pub adapter: String,
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
        // The driver paces frames with wl_surface frame callbacks in its own loop. A present
        // that waited for the compositor as well (Fifo) would block that loop, and with it input
        // and the accessibility bus, for up to a second behind a covered window.
        let present_mode = if capabilities.present_modes.contains(&PresentMode::Mailbox) {
            PresentMode::Mailbox
        } else {
            PresentMode::AutoNoVsync
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
            resources,
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
        paint(ui, scale, &mut self.scene, &mut self.resources)?;
        let frame = match self.surface.get_current_texture() {
            CurrentSurfaceTexture::Success(frame) | CurrentSurfaceTexture::Suboptimal(frame) => {
                frame
            }
            other => return Err(format!("the window's surface has no frame: {other:?}").into()),
        };
        let view = frame.texture.create_view(&Default::default());
        let mut encoder = self.device.create_command_encoder(&Default::default());
        self.renderer.render(
            &self.scene,
            &mut self.resources,
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
