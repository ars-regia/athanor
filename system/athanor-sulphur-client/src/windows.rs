//! The windows: the standard toplevel list, and the state of each window through
//! athanor-sulphur's toplevel manager or cosmic-comp's toplevel info. This crate binds them at
//! the first watch (`Client::watch_windows`), so a client that never watches the windows, as the
//! OSD (doc_osd.md OD8), never binds them and its window model stays empty. Every name of the
//! privileged ones in this crate is in this file.

use wayland_client::globals::GlobalList;
use wayland_client::{
    delegate_noop, event_created_child, Connection, Dispatch, Proxy, QueueHandle,
};
use wayland_protocols::ext::foreign_toplevel_list::v1::client::{
    ext_foreign_toplevel_handle_v1::ExtForeignToplevelHandleV1,
    ext_foreign_toplevel_list_v1::{self, ExtForeignToplevelListV1},
};

use crate::connection::{bind, fresh, Client, Error, State, Toplevel};
use crate::model::{Event, Window, WindowId, WindowState};
use crate::protocols::toplevel_info::client::zcosmic_toplevel_info_v1::ZcosmicToplevelInfoV1;
use crate::protocols::toplevel_state::client::athanor_toplevel_manager_v1::AthanorToplevelManagerV1;

/// The toplevel list and the managers of the window state, bound at the first watch. Where the
/// compositor does not offer the list, they are looked for again at the next watch.
#[derive(Default)]
pub(crate) struct Globals {
    list: Option<ExtForeignToplevelListV1>,
    cosmic: Option<ZcosmicToplevelInfoV1>,
    athanor: Option<AthanorToplevelManagerV1>,
}

/// Whether `globals` offers athanor-sulphur's toplevel manager, which only it offers and only
/// to the shell (doc_own_stack.md, decision 28), bound or not.
pub(crate) fn own_compositor(globals: &GlobalList) -> bool {
    let name = AthanorToplevelManagerV1::interface().name;
    globals
        .contents()
        .with_list(|list| list.iter().any(|global| global.interface == name))
}

impl Client {
    /// Follows the windows (`Client::windows`, `Event::WindowAdded` and the others): binds the
    /// toplevel list and the manager of the window state, and reads the windows already open
    /// as their initial state, which no event reports, as a connection reads its own. Nothing to
    /// do where the list is already bound. A compositor that offers no toplevel list, as when a
    /// security context's grant hides it, leaves the windows empty and is no error.
    pub fn watch_windows(&self) -> Result<(), Error> {
        self.live()?;
        {
            let mut state = self.inner.state.borrow_mut();
            let globals = &mut state.globals.windows;
            if globals.list.is_some() {
                return Ok(());
            }
            let (registry, qh) = (&self.inner.registry, &self.inner.qh);
            let Some(list) = bind::<ExtForeignToplevelListV1>(registry, qh, 1..=1) else {
                return Ok(());
            };
            globals.cosmic = bind(registry, qh, 2..=3);
            globals.athanor = bind(registry, qh, 1..=1);
            globals.list = Some(list);
        }
        self.announce()?;
        self.inner.state.borrow_mut().events.retain(|event| {
            !matches!(
                event,
                Event::WindowAdded(_) | Event::WindowChanged(_) | Event::WindowRemoved(_)
            )
        });
        Ok(())
    }
}

impl Dispatch<ExtForeignToplevelListV1, ()> for State {
    fn event(
        state: &mut Self,
        _: &ExtForeignToplevelListV1,
        event: ext_foreign_toplevel_list_v1::Event,
        _: &(),
        _: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        let ext_foreign_toplevel_list_v1::Event::Toplevel { toplevel } = event else {
            return;
        };
        let Some(&id) = toplevel.data::<WindowId>() else {
            return;
        };
        let globals = &state.globals.windows;
        let cosmic = globals
            .cosmic
            .as_ref()
            .map(|info| info.get_cosmic_toplevel(&toplevel, qh, id));
        // COSMIC's extension first (plan D3). One state object per window: the compositor
        // ends the connection on a second.
        let athanor = match (&cosmic, &globals.athanor) {
            (None, Some(manager)) => Some(manager.get_toplevel_state(&toplevel, qh, id)),
            _ => None,
        };
        state.windows.insert(
            id,
            Window {
                id,
                app_id: String::new(),
                title: String::new(),
                state: WindowState::default(),
                outputs: Vec::new(),
            },
        );
        state.toplevels.insert(
            id,
            Toplevel {
                ext: toplevel,
                cosmic,
                athanor,
            },
        );
    }

    event_created_child!(State, ExtForeignToplevelListV1, [
        ext_foreign_toplevel_list_v1::EVT_TOPLEVEL_OPCODE => (ExtForeignToplevelHandleV1, WindowId(fresh()))
    ]);
}

delegate_noop!(State: ignore AthanorToplevelManagerV1);

#[cfg(test)]
pub(crate) mod tests {
    use crate::connection::Client;
    use std::io::{Read, Write};
    use std::os::unix::net::UnixStream;
    use std::sync::{Arc, Mutex};
    use wayland_client::Connection;

    pub(crate) const TOPLEVEL_LIST: &str = "ext_foreign_toplevel_list_v1";
    const TOPLEVEL_MANAGER: &str = "athanor_toplevel_manager_v1";

    fn message(object: u32, opcode: u32, args: &[u8]) -> Vec<u8> {
        let size = u32::try_from(8 + args.len()).expect("size");
        let mut out = object.to_ne_bytes().to_vec();
        out.extend((size << 16 | opcode).to_ne_bytes());
        out.extend(args);
        out
    }

    fn string(text: &str) -> Vec<u8> {
        let mut out = u32::try_from(text.len() + 1)
            .expect("length")
            .to_ne_bytes()
            .to_vec();
        out.extend(text.as_bytes());
        out.push(0);
        out.resize(out.len().next_multiple_of(4), 0);
        out
    }

    /// A compositor speaking the wire protocol: it offers `offered`, answers each sync, records
    /// the interface of every bind, and announces one window on the toplevel list, until the
    /// client hangs up.
    fn serve(mut stream: UnixStream, offered: &[&str], bound: &Mutex<Vec<String>>) {
        let word = |bytes: &[u8], at: usize| {
            u32::from_ne_bytes(bytes[at..at + 4].try_into().expect("word"))
        };
        let mut registry = None;
        let mut header = [0; 8];
        while stream.read_exact(&mut header).is_ok() {
            let (object, size_opcode) = (word(&header, 0), word(&header, 4));
            let mut body = vec![0; (size_opcode >> 16) as usize - 8];
            stream.read_exact(&mut body).expect("body");
            let mut reply = Vec::new();
            match (object, size_opcode & 0xffff) {
                // wl_display.get_registry: the globals.
                (1, 1) => {
                    let id = word(&body, 0);
                    registry = Some(id);
                    for (name, interface) in (1u32..).zip(offered) {
                        let version = 1u32.to_ne_bytes();
                        let args = [&name.to_ne_bytes()[..], &string(interface), &version].concat();
                        reply.extend(message(id, 0, &args));
                    }
                }
                // wl_display.sync: wl_callback.done, then wl_display.delete_id.
                (1, 0) => {
                    let callback = word(&body, 0);
                    reply.extend(message(callback, 0, &0u32.to_ne_bytes()));
                    reply.extend(message(1, 1, &callback.to_ne_bytes()));
                }
                // wl_registry.bind: name, then the interface as a string.
                (id, 0) if Some(id) == registry => {
                    let length = word(&body, 4) as usize;
                    let interface = String::from_utf8_lossy(&body[8..8 + length - 1]);
                    if interface == TOPLEVEL_LIST {
                        // ext_foreign_toplevel_list_v1.toplevel, a server-allocated handle,
                        // then its ext_foreign_toplevel_handle_v1.done.
                        let list = word(&body, 8 + length.next_multiple_of(4) + 4);
                        reply.extend(message(list, 0, &0xff00_0000u32.to_ne_bytes()));
                        reply.extend(message(0xff00_0000, 1, &[]));
                    }
                    bound.lock().expect("lock").push(interface.into_owned());
                }
                _ => {}
            }
            stream.write_all(&reply).expect("reply");
        }
    }

    /// A client connected to a compositor that offers `offered`, and the interfaces it binds.
    pub(crate) fn connect(
        offered: &'static [&'static str],
    ) -> (Connection, Client, Arc<Mutex<Vec<String>>>) {
        let (ours, theirs) = UnixStream::pair().expect("socket pair");
        let bound = Arc::new(Mutex::new(Vec::new()));
        let record = bound.clone();
        std::thread::spawn(move || serve(theirs, offered, &record));
        let connection = Connection::from_socket(ours).expect("connection");
        let client = Client::connect_wayland(&connection).expect("client");
        (connection, client, bound)
    }

    /// Least privilege (doc_osd.md OD8): a client that never watches the windows binds neither
    /// window global, even where the compositor offers both, and its window model stays empty.
    #[test]
    fn connecting_binds_no_window_global() {
        let _context = crate::MAIN_CONTEXT
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let (_connection, client, bound) = connect(&[TOPLEVEL_LIST, TOPLEVEL_MANAGER]);
        let bound = bound.lock().expect("lock").clone();
        assert!(
            !bound
                .iter()
                .any(|i| i == TOPLEVEL_LIST || i == TOPLEVEL_MANAGER),
            "{bound:?}"
        );
        assert!(client.windows().is_empty());
        assert!(
            client.own_compositor(),
            "athanor_toplevel_manager_v1 is offered"
        );
    }

    /// A client that watches the windows binds both, once across its watches, and has the
    /// window already open in its model at once, as its initial state and not as an event.
    #[test]
    fn watching_the_windows_binds_both_once_and_reads_the_open_window() {
        let _context = crate::MAIN_CONTEXT
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let (_connection, client, bound) = connect(&[TOPLEVEL_LIST, TOPLEVEL_MANAGER]);
        for _ in 0..2 {
            client.watch_windows().expect("watch the windows");
        }
        assert_eq!(client.windows().len(), 1);
        assert!(client.inner.state.borrow().events.is_empty());
        let bound = bound.lock().expect("lock").clone();
        for global in [TOPLEVEL_LIST, TOPLEVEL_MANAGER] {
            assert_eq!(
                bound.iter().filter(|i| *i == global).count(),
                1,
                "{bound:?}"
            );
        }
    }

    /// A security context's grant can hide the window globals: watching then binds nothing,
    /// fails nothing, and leaves the windows empty.
    #[test]
    fn watching_windows_the_compositor_hides_leaves_them_empty() {
        let _context = crate::MAIN_CONTEXT
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let (_connection, client, bound) = connect(&[]);
        client
            .watch_windows()
            .expect("no error without the window globals");
        assert!(client.windows().is_empty());
        assert!(!client.own_compositor());
        assert!(bound.lock().expect("lock").is_empty());
    }
}
