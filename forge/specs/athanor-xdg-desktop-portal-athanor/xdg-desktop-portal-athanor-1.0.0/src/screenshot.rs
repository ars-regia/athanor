//! The outputs' pictures for the Screenshot interface (doc_portal.md, PT8). The compositor's
//! client lives on the GLib main thread; the D-Bus thread asks it there and awaits the answer.

use std::cell::RefCell;
use std::path::PathBuf;

use athanor_portal::screenshot::{Format, Host, Shot};
use athanor_sulphur_client::{Client, OutputPicture, PixelFormat};
use tokio::sync::oneshot;
use zbus::message::Header;

thread_local! {
    /// The compositor's client, made on the main thread at the first screenshot and kept for
    /// the next; a capture that fails drops it, and the next screenshot connects again.
    static CLIENT: RefCell<Option<Client>> = const { RefCell::new(None) };
}

/// The backend's side of the interface.
pub struct Outputs {
    /// The connection the frontend check asks the bus on (PT3, `caller::authorise`).
    query: zbus::Connection,
    /// Where the capture buffers are created: the unit's runtime directory, the one place
    /// beneath `$XDG_RUNTIME_DIR` Landlock lets the backend write (PT3).
    dir: PathBuf,
}

impl Outputs {
    pub fn new(query: zbus::Connection, dir: PathBuf) -> Outputs {
        Outputs { query, dir }
    }
}

impl Host for Outputs {
    async fn frontend(&self, header: &Header<'_>) -> Result<(), String> {
        crate::caller::authorise(header, &self.query)
            .await
            .map_err(|err| err.to_string())
    }

    async fn capture(&self) -> Result<Vec<Shot>, String> {
        let (answer, answered) = oneshot::channel();
        let dir = self.dir.clone();
        // An idle source runs only in an iteration of the default context, on the main
        // thread, where `CLIENT` lives; `invoke` could run in place on this thread if the main
        // loop did not own the context yet.
        glib::idle_add_once(move || {
            glib::MainContext::default().spawn_local(async move {
                if answer.send(take(dir).await).is_err() {
                    tracing::info!("the screenshot was closed before its pictures were taken");
                }
            });
        });
        answered
            .await
            .map_err(|_| "the main loop dropped the capture".to_owned())?
    }
}

/// Every output's picture, on the main thread.
async fn take(dir: PathBuf) -> Result<Vec<Shot>, String> {
    let client = CLIENT.with_borrow_mut(|kept| match kept.take() {
        Some(client) if !client.is_closed() => Ok(client),
        _ => connect(dir),
    })?;
    let taken = client.capture_outputs().await;
    match taken {
        Ok(outputs) => {
            CLIENT.with_borrow_mut(|kept| *kept = Some(client));
            Ok(outputs.into_iter().map(shot).collect())
        }
        Err(err) => Err(format!("the outputs were not captured: {err}")),
    }
}

fn connect(dir: PathBuf) -> Result<Client, String> {
    let connection = wayland_client::Connection::connect_to_env()
        .map_err(|err| format!("cannot connect to the compositor: {err}"))?;
    let client = Client::connect_wayland(&connection)
        .map_err(|err| format!("cannot connect to the compositor: {err}"))?;
    client.set_capture_dir(dir);
    Ok(client)
}

fn shot(output: OutputPicture) -> Shot {
    let picture = output.picture;
    Shot {
        x: output.x,
        y: output.y,
        width: output.width,
        height: output.height,
        transform: output.transform,
        pixels_width: picture.width,
        pixels_height: picture.height,
        stride: picture.stride,
        format: match picture.format {
            PixelFormat::Argb8888 => Format::Argb8888,
            PixelFormat::Xrgb8888 => Format::Xrgb8888,
            PixelFormat::Abgr8888 => Format::Abgr8888,
            PixelFormat::Xbgr8888 => Format::Xbgr8888,
        },
        bytes: picture.bytes,
    }
}
