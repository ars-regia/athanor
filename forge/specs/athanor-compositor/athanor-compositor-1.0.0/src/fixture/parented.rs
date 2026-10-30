//! The fixture's tests of a dialog with a parent (`follow_parent`): a toplevel whose parent is
//! set through xdg-foreign or `xdg_toplevel.set_parent` is stacked above the parent, and takes
//! the keyboard when the parent's client holds it.

use super::*;
use wayland_protocols::xdg::foreign::zv2::client::{
    zxdg_exporter_v2, zxdg_imported_v2, zxdg_importer_v2,
};

/// A toplevel of `Local::create`.
struct Created {
    surface: wl_surface::WlSurface,
    toplevel: xdg_toplevel::XdgToplevel,
    _xdg: xdg_surface::XdgSurface,
}

impl Local {
    /// A toplevel with its first configure acknowledged and no buffer yet, so that a parent can
    /// be set before the compositor places it.
    fn create(&mut self, run: &mut Running) -> Created {
        let compositor: wl_compositor::WlCompositor = self.bind(4);
        let wm_base: xdg_wm_base::XdgWmBase = self.bind(1);
        let qh = self.queue.handle();
        let surface = compositor.create_surface(&qh, ());
        let xdg = wm_base.get_xdg_surface(&surface, &qh, ());
        let toplevel = xdg.get_toplevel(&qh, ());
        self.client.configure = None;
        surface.commit();
        self.pump(run, "the window is configured", |client, _| {
            client.configure.is_some()
        });
        xdg.ack_configure(self.client.configure.take().expect("a configure"));
        Created {
            surface,
            toplevel,
            _xdg: xdg,
        }
    }

    /// Draws the first buffer of `created`: `place` decides its place and focus.
    fn map(&mut self, run: &mut Running, created: &Created) -> smithay::desktop::Window {
        let shm: wl_shm::WlShm = self.bind(1);
        let (buffer, _) = self.buffer(&shm, WIDTH, HEIGHT, [0, 0, 0, 255]);
        created.surface.attach(Some(&buffer), 0, 0);
        created.surface.commit();
        self.roundtrip(run);
        self.client.configure = None;
        self.server_window(run, &created.surface)
    }

    /// The handle of the exported toplevel `surface`.
    fn export(&mut self, run: &mut Running, surface: &wl_surface::WlSurface) -> String {
        let exporter: zxdg_exporter_v2::ZxdgExporterV2 = self.bind(1);
        self.client.devices.handle = None;
        let _exported = exporter.export_toplevel(surface, &self.queue.handle(), ());
        self.pump(run, "a handle", |client, _| client.devices.handle.is_some());
        self.client.devices.handle.clone().expect("a handle")
    }

    /// Makes the exported toplevel `handle` the parent of `child`.
    fn import_parent(
        &mut self,
        run: &mut Running,
        handle: String,
        child: &wl_surface::WlSurface,
    ) -> zxdg_imported_v2::ZxdgImportedV2 {
        let importer: zxdg_importer_v2::ZxdgImporterV2 = self.bind(1);
        let imported = importer.import_toplevel(handle, &self.queue.handle(), ());
        imported.set_parent_of(child);
        self.roundtrip(run);
        imported
    }
}

/// The place of `window` in the stack, from the bottom.
fn level(run: &Running, window: &smithay::desktop::Window) -> usize {
    stack(run)
        .iter()
        .position(|kept| kept == window)
        .expect("in the stack")
}

/// A client with a window that holds the keyboard, that window and its surface, and a second
/// client with no window.
fn scene(
    run: &mut Running,
) -> (
    Local,
    xdg_toplevel::XdgToplevel,
    wl_surface::WlSurface,
    smithay::desktop::Window,
    Local,
) {
    let mut owner = Local::connect(run, None);
    let toplevel = owner.toplevel(run);
    let surface = owner.window.as_ref().expect("the window").0.clone();
    let parent = owner.server_window(run, &surface);
    assert!(keyboard_on(run, &parent), "the parent holds the keyboard");
    let dialog = Local::connect(run, None);
    (owner, toplevel, surface, parent, dialog)
}

/// A parent imported before the dialog is placed: the dialog is above it and has the keyboard.
#[test]
fn a_dialog_parented_through_xdg_foreign_takes_the_keyboard_of_its_parent() {
    let mut run = Running::start(None);
    let (mut owner, _toplevel, surface, parent, mut dialog) = scene(&mut run);
    let handle = owner.export(&mut run, &surface);
    let created = dialog.create(&mut run);
    let _imported = dialog.import_parent(&mut run, handle, &created.surface);
    let window = dialog.map(&mut run, &created);
    assert!(keyboard_on(&run, &window), "the dialog has the keyboard");
    assert!(
        level(&run, &window) > level(&run, &parent),
        "above the parent"
    );
    run.stop();
}

/// A parent imported after the dialog is placed: it was beneath, and moves above with the keyboard.
#[test]
fn a_parent_imported_after_the_dialog_is_placed_raises_it() {
    let mut run = Running::start(None);
    let (mut owner, _toplevel, surface, parent, mut dialog) = scene(&mut run);
    let handle = owner.export(&mut run, &surface);
    let created = dialog.create(&mut run);
    let window = dialog.map(&mut run, &created);
    assert!(!keyboard_on(&run, &window), "no parent, no keyboard");
    assert!(
        level(&run, &window) < level(&run, &parent),
        "beneath the parent"
    );
    let _imported = dialog.import_parent(&mut run, handle, &created.surface);
    assert!(keyboard_on(&run, &window), "the dialog has the keyboard");
    assert!(
        level(&run, &window) > level(&run, &parent),
        "above the parent"
    );
    run.stop();
}

/// A window without a parent keeps the rule: mapping does not take the keyboard from another client.
#[test]
fn a_window_without_a_parent_stays_beneath() {
    let mut run = Running::start(None);
    let (_owner, _toplevel, _surface, parent, mut dialog) = scene(&mut run);
    let created = dialog.create(&mut run);
    let window = dialog.map(&mut run, &created);
    assert!(keyboard_on(&run, &parent), "the parent keeps the keyboard");
    assert!(
        level(&run, &window) < level(&run, &parent),
        "beneath the parent"
    );
    run.stop();
}

/// The parent's client does not hold the keyboard: the dialog is just above the parent, under
/// the window that holds the keyboard, and has no keyboard.
#[test]
fn a_dialog_does_not_take_a_keyboard_the_parents_client_does_not_hold() {
    let mut run = Running::start(None);
    let (mut owner, _toplevel, surface, parent, mut dialog) = scene(&mut run);
    let (_typing, typed) = focused_window(&mut run);
    assert!(
        keyboard_on(&run, &typed),
        "another client holds the keyboard"
    );
    let handle = owner.export(&mut run, &surface);
    let created = dialog.create(&mut run);
    let _imported = dialog.import_parent(&mut run, handle, &created.surface);
    let window = dialog.map(&mut run, &created);
    assert!(keyboard_on(&run, &typed), "the keyboard stays");
    assert!(
        level(&run, &parent) < level(&run, &window) && level(&run, &window) < level(&run, &typed),
        "just above the parent, under the window typed into"
    );
    run.stop();
}

/// `xdg_toplevel.set_parent` stacks the same way: just above the parent, and no keyboard that
/// the parent's client does not hold.
#[test]
fn a_native_child_is_above_its_parent() {
    let mut run = Running::start(None);
    let (mut owner, toplevel, _surface, parent, _dialog) = scene(&mut run);
    let (_typing, typed) = focused_window(&mut run);
    let created = owner.create(&mut run);
    created.toplevel.set_parent(Some(&toplevel));
    let window = owner.map(&mut run, &created);
    assert!(keyboard_on(&run, &typed), "the keyboard stays");
    assert!(
        level(&run, &parent) < level(&run, &window) && level(&run, &window) < level(&run, &typed),
        "just above the parent, under the window typed into"
    );
    run.stop();
}

/// A parent that is minimised is not shown: its client holds the keyboard through another
/// window, and the dialog neither takes it nor leaves the bottom.
#[test]
fn a_dialog_of_a_minimised_parent_takes_nothing() {
    let mut run = Running::start(None);
    let (mut owner, _toplevel, surface, parent, mut dialog) = scene(&mut run);
    let other = owner.another(&mut run, "other");
    let other = owner.server_window(&run, &other.surface);
    assert!(
        keyboard_on(&run, &other),
        "the client's other window has the keyboard"
    );
    assert!(run.state.minimise(&parent), "minimised");
    let handle = owner.export(&mut run, &surface);
    let created = dialog.create(&mut run);
    let _imported = dialog.import_parent(&mut run, handle, &created.surface);
    let window = dialog.map(&mut run, &created);
    assert!(keyboard_on(&run, &other), "the keyboard stays");
    assert_eq!(level(&run, &window), 0, "beneath everything");
    run.stop();
}
