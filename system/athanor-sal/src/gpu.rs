//! Drawing on vello_gpu: the scene of `render.rs`, rasterised through wgpu straight onto the
//! window's wl_surface (doc_own_stack.md section 7, the renderer milestone T1 measures).
//!
//! wgpu takes the window as libwayland-client's raw pointers, which no safe interface hands
//! over: with text/fontconfig.rs, this is one of the two modules of the crate with `unsafe` code.

#![allow(unsafe_code)]

use crate::render::{layout, paint, Error};
use crate::widget::{Kind, Ui};
use std::future::Future;
use std::pin::pin;
use std::ptr::NonNull;
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll, Waker};
use vello_gpu::{
    ImageSource, Pixmap, RenderOptions, RenderTarget, RenderTargetConfig, TextureBindings,
    TextureId, TextureRegion,
};
use wayland_client::{protocol::wl_surface::WlSurface, Connection, Proxy};
use wgpu::rwh::{HasDisplayHandle, RawWindowHandle, WaylandWindowHandle};
use wgpu::{CurrentSurfaceTexture, PresentMode, TextureFormat};

/// The GPU renderer of one Wayland surface, drawing with vello_gpu through wgpu.
pub struct Gpu {
    surface: wgpu::Surface<'static>,
    config: wgpu::SurfaceConfiguration,
    painter: Painter,
    /// The adapter wgpu picked, as the start-up line names it.
    pub adapter: String,
}

/// What draws a window on a wgpu device, whatever texture the frame goes to.
struct Painter {
    device: wgpu::Device,
    queue: wgpu::Queue,
    renderer: vello_gpu::Renderer,
    resources: Resources,
    scene: vello_gpu::Scene,
    /// The first error wgpu reported outside a call's result, which the next frame returns.
    errors: Arc<Mutex<Option<String>>>,
    /// The largest side of a texture on `device`.
    max_side: u32,
}

/// Whether `pixmap` fits a texture of a device whose textures are at most `max_side` on a side.
/// wgpu fails a larger one, and with it every frame that binds it.
fn fits(pixmap: &Pixmap, max_side: u32) -> bool {
    u32::from(pixmap.width()) <= max_side && u32::from(pixmap.height()) <= max_side
}

/// vello_gpu's resources, and the images of the window, each in a wgpu texture of its own.
/// vello_gpu panics on an image paint that carries its pixmap ("pixmap image sources are not
/// supported by Vello GPU"): an image is drawn from a texture bound by its id.
pub struct Resources {
    vello: vello_gpu::Resources,
    /// Each uploaded pixmap with the id its texture is bound by. The `Arc` is held, so that the
    /// address it is matched by is not reused while it is listed.
    images: Vec<(Arc<Pixmap>, TextureId)>,
    /// The texture of each id of `images`; a view holds its texture.
    bindings: TextureBindings,
    /// The id the next upload takes.
    next: u64,
    /// The pixmaps left out at the last frame for want of room, so that each is logged once.
    skipped: Vec<Arc<Pixmap>>,
}

impl Resources {
    /// The uploaded copy of `pixmap`, if there is one.
    pub(crate) fn image(&self, pixmap: &Arc<Pixmap>) -> Option<ImageSource> {
        self.images
            .iter()
            .find(|(uploaded, _)| Arc::ptr_eq(uploaded, pixmap))
            .map(|(uploaded, id)| {
                ImageSource::external_texture(*id, TextureRegion::Full, uploaded.may_have_transparency())
            })
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
        Ok(Self {
            surface,
            config,
            painter: Painter::new(&adapter, format)?,
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
            self.surface.configure(&self.painter.device, &self.config);
        }
        let frame = match self.surface.get_current_texture() {
            CurrentSurfaceTexture::Success(frame) | CurrentSurfaceTexture::Suboptimal(frame) => {
                frame
            }
            other => return Err(format!("the window's surface has no frame: {other:?}").into()),
        };
        let view = frame.texture.create_view(&Default::default());
        self.painter.draw(ui, scale, (width, height), &view)?;
        self.painter.queue.present(frame);
        // A present that fails reaches the error slot; one that ends Outdated, Lost, Timeout or
        // Occluded does not (wgpu 30 drops that status), and may commit nothing: the driver
        // commits the surface after this, so its frame callback comes in either case.
        self.painter.check()?;
        Ok((width, height))
    }
}

impl Painter {
    /// Opens a device on `adapter` and vello_gpu's renderer, drawing in `format`.
    fn new(adapter: &wgpu::Adapter, format: TextureFormat) -> Result<Self, Error> {
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
        let max_side = device.limits().max_texture_dimension_2d;
        let (renderer, resources) = vello_gpu::Renderer::new(
            &device,
            &queue,
            &RenderTargetConfig {
                format,
                width: 1,
                height: 1,
            },
        );
        Ok(Self {
            device,
            queue,
            renderer,
            resources: Resources {
                vello: resources,
                images: Vec::new(),
                bindings: TextureBindings::new(),
                next: 0,
                skipped: Vec::new(),
            },
            scene: vello_gpu::Scene::new(1, 1),
            errors,
            max_side,
        })
    }

    /// Draws `ui`, laid out at `scale` to `width` by `height`, onto `view`, and submits it.
    fn draw(
        &mut self,
        ui: &mut Ui,
        scale: f32,
        (width, height): (u16, u16),
        view: &wgpu::TextureView,
    ) -> Result<(), Error> {
        self.scene.reset_and_resize(width, height);
        self.upload_images(ui);
        paint(ui, scale, &mut self.scene, &mut self.resources)?;
        let mut encoder = self.device.create_command_encoder(&Default::default());
        // The default options clear the target first, as each frame draws all of it.
        self.renderer.render(
            &self.scene,
            &mut self.resources.vello,
            &mut encoder,
            RenderTarget {
                view,
                depth_view: None,
            },
            &self.resources.bindings,
            RenderOptions::default(),
        )?;
        self.queue.submit([encoder.finish()]);
        self.check()
    }

    /// Uploads each image of `ui`'s widgets that has no texture yet to a texture of its own, and
    /// releases the textures of those no widget holds any more. An image larger on a side than
    /// the device's textures is left out, and logged.
    fn upload_images(&mut self, ui: &Ui) {
        let wanted: Vec<&Arc<Pixmap>> = ui
            .ids()
            .flat_map(|id| match &ui.widget(id).kind {
                Kind::Image { image, .. }
                | Kind::IconButton { image, .. }
                | Kind::LabelledButton {
                    image: Some(image), ..
                }
                | Kind::IconToggle(crate::icon_toggle::IconToggle { image, .. })
                | Kind::Picture { image, .. } => vec![image.pixmap()],
                // Both of a tile's glyphs, so that turning it over draws one already uploaded.
                Kind::Tile(tile) => tile.glyphs.iter().flat_map(|g| &g.tinted).map(|i| i.pixmap()).collect(),
                _ => Vec::new(),
            })
            .collect();
        let resources = &mut self.resources;
        let (kept, gone): (Vec<_>, Vec<_>) = std::mem::take(&mut resources.images)
            .into_iter()
            .partition(|(uploaded, _)| wanted.iter().any(|w| Arc::ptr_eq(w, uploaded)));
        resources.images = kept;
        for (_, id) in gone {
            // The view held the texture: wgpu frees it once no submitted frame reads it.
            resources.bindings.remove(id);
        }
        let mut skipped = Vec::new();
        for pixmap in wanted {
            if resources.image(pixmap).is_some() {
                continue;
            }
            let (width, height) = (u32::from(pixmap.width()), u32::from(pixmap.height()));
            if !fits(pixmap, self.max_side) {
                // Left out, so not drawn; the rest of the window is.
                if !resources.skipped.iter().any(|s| Arc::ptr_eq(s, pixmap)) {
                    eprintln!(
                        "athanor-sal: an image of {width}x{height} pixels is not drawn: the GPU's \
                         textures are at most {} pixels on a side",
                        self.max_side
                    );
                }
                skipped.push(Arc::clone(pixmap));
                continue;
            }
            let size = wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            };
            // Premultiplied RGBA8, as the pixmap holds it and vello_gpu samples it.
            let texture = self.device.create_texture(&wgpu::TextureDescriptor {
                label: None,
                size,
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: TextureFormat::Rgba8Unorm,
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            });
            self.queue.write_texture(
                texture.as_image_copy(),
                pixmap.data_as_u8_slice(),
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(width * 4),
                    rows_per_image: None,
                },
                size,
            );
            let id = TextureId(resources.next);
            resources.next += 1;
            resources
                .bindings
                .insert(id, texture.create_view(&Default::default()));
            resources.images.push((Arc::clone(pixmap), id));
        }
        resources.skipped = skipped;
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::icon::Image;
    use crate::Theme;
    use std::sync::mpsc;
    use vello_cpu::peniko::ImageAlphaType;
    use vello_cpu::PixelMetadata;

    /// Draws a widget of `kind` alone on `painter`'s device and reads back the pixel at its
    /// centre.
    fn centre_of(painter: &mut Painter, kind: Kind) -> Result<[u8; 4], Error> {
        let mut ui = Ui::new("Image", Theme::default(), false);
        let id = ui.add(Ui::ROOT, kind);
        let (width, height) = layout(&mut ui, 1.0)?;
        let size = wgpu::Extent3d {
            width: u32::from(width),
            height: u32::from(height),
            depth_or_array_layers: 1,
        };
        let texture = painter.device.create_texture(&wgpu::TextureDescriptor {
            label: None,
            size,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let view = texture.create_view(&Default::default());
        painter.draw(&mut ui, 1.0, (width, height), &view)?;
        // A row of a copy to a buffer is a multiple of 256 bytes.
        let row = (size.width * 4).next_multiple_of(256);
        let buffer = painter.device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: u64::from(row * size.height),
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut encoder = painter.device.create_command_encoder(&Default::default());
        encoder.copy_texture_to_buffer(
            texture.as_image_copy(),
            wgpu::TexelCopyBufferInfo {
                buffer: &buffer,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(row),
                    rows_per_image: None,
                },
            },
            size,
        );
        painter.queue.submit([encoder.finish()]);
        let (sender, mapped) = mpsc::channel();
        buffer.map_async(wgpu::MapMode::Read, .., move |result| {
            let _ = sender.send(result);
        });
        painter.device.poll(wgpu::PollType::wait_indefinitely())?;
        mapped.recv()??;
        painter.check()?;
        let r = ui.widget(id).rect;
        let (x, y) = ((r.x + r.w / 2.0) as u32, (r.y + r.h / 2.0) as u32);
        let at = usize::try_from(y * row + x * 4)?;
        let pixels = buffer.get_mapped_range(..)?;
        let pixel = pixels
            .get(at..at + 4)
            .ok_or("the centre is outside the frame")?;
        Ok([pixel[0], pixel[1], pixel[2], pixel[3]])
    }

    /// A picture wider or taller than the device's textures is left out instead of reaching
    /// wgpu, which would fail the frame; one at the limit is uploaded.
    #[test]
    fn a_picture_larger_than_the_device_allows_is_not_uploaded() -> Result<(), Error> {
        let wide = Image::from_rgba8(4097, 1, vec![0; 4097 * 4])?;
        assert!(!fits(wide.pixmap(), 4096));
        let tall = Image::from_rgba8(1, 4097, vec![0; 4097 * 4])?;
        assert!(!fits(tall.pixmap(), 4096));
        let edge = Image::from_rgba8(4096, 1, vec![0; 4096 * 4])?;
        assert!(fits(edge.pixmap(), 4096));
        assert!(!fits(edge.pixmap(), 2048));
        Ok(())
    }

    fn image(image: Image) -> Kind {
        Kind::Image {
            image,
            size: 32.0,
            description: String::new(),
        }
    }

    fn solid(rgba: [u8; 4]) -> Image {
        let metadata = PixelMetadata::new(ImageAlphaType::AlphaPremultiplied, false);
        Image {
            pixmap: Arc::new(Pixmap::from_parts(rgba.repeat(32 * 32), 32, 32, metadata)),
        }
    }

    /// The image of a widget replaced by another of the same size: the first's texture is
    /// released, and the second is drawn.
    #[test]
    #[ignore = "needs a wgpu adapter: run with --features gpu -- --ignored where one exists"]
    fn an_image_replaced_by_another_is_drawn() -> Result<(), Error> {
        let instance =
            wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env());
        // No adapter fails the test: it runs only where one was asked for.
        let adapter = ready(instance.request_adapter(&Default::default()))??;
        eprintln!("adapter: {:?}", adapter.get_info());
        let mut painter = Painter::new(&adapter, TextureFormat::Rgba8Unorm)?;
        let red = [0xcc, 0, 0, 0xff];
        let blue = [0, 0, 0xcc, 0xff];
        assert_eq!(centre_of(&mut painter, image(solid(red)))?, red);
        assert_eq!(centre_of(&mut painter, image(solid(blue)))?, blue);
        assert_eq!(painter.resources.images.len(), 1);
        Ok(())
    }

    /// An icon toggle's image is uploaded, as an icon button's is, and is drawn.
    #[test]
    #[ignore = "needs a wgpu adapter: run with --features gpu -- --ignored where one exists"]
    fn an_icon_toggles_image_is_uploaded_and_drawn() -> Result<(), Error> {
        let instance =
            wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env());
        let adapter = ready(instance.request_adapter(&Default::default()))??;
        let mut painter = Painter::new(&adapter, TextureFormat::Rgba8Unorm)?;
        let red = [0xcc, 0, 0, 0xff];
        let toggle = Kind::IconToggle(crate::icon_toggle::IconToggle {
            label: "Do Not Disturb".into(),
            image: solid(red),
            size: 32.0,
            active: false,
        });
        assert_eq!(centre_of(&mut painter, toggle)?, red);
        assert_eq!(painter.resources.images.len(), 1);
        Ok(())
    }

    /// A tile's glyphs, off and on, are uploaded, as an icon button's image is. The glyph
    /// sits in the disc, off the tile's centre, so only the upload is read.
    #[test]
    #[ignore = "needs a wgpu adapter: run with --features gpu -- --ignored where one exists"]
    fn a_tiles_glyphs_are_uploaded() -> Result<(), Error> {
        let instance =
            wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env());
        let adapter = ready(instance.request_adapter(&Default::default()))??;
        let mut painter = Painter::new(&adapter, TextureFormat::Rgba8Unorm)?;
        let tile = Kind::Tile(crate::tile::Tile {
            title: "Wi-Fi".into(),
            subtitle: "Home".into(),
            image: solid([0xcc, 0, 0, 0xff]),
            active: true,
            second: None,
            placed: None,
            glyphs: None,
        });
        centre_of(&mut painter, tile)?;
        assert_eq!(painter.resources.images.len(), 2);
        Ok(())
    }
}
