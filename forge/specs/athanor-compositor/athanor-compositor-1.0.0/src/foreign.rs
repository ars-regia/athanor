//! zxdg_exporter_v2 and zxdg_importer_v2 (doc_own_stack.md OS17 and decision 30, standard): a
//! client exports a toplevel as a handle, another imports the handle and sets its own toplevel
//! as a child of it, which is what `xdg_toplevel.set_parent` does: a portal's file chooser
//! opens as a dialog of the window that asked for it. A handle is valid while its exported
//! object and its exported toplevel live; when either goes, the relationships it made end and
//! every imported object is told (`destroyed`).
//!
//! Smithay's `xdg_foreign` serves the same protocol but revokes a handle only when the exported
//! object is destroyed, not when the toplevel is, and keeps one child per handle; this module
//! revokes in both cases and keeps every child. It is a port of that module, under the MIT
//! licence below.
//!
//! Copyright (c) 2017 Victor Berger and Victoria Brekenfeld (Smithay, MIT)
//! Source: Smithay revision 023b78b1c5a05ca512767cacc07efca55fae8be4,
//! `src/wayland/xdg_foreign/{mod,handlers}.rs`.
//! Changed by the Athanor authors on 2026-10-29: the state is the compositor's own, the handle
//! is read from /dev/urandom, the handle is revoked when the exported toplevel is destroyed,
//! and every child of a handle is kept.
//!
//! The MIT License
//!
//! Permission is hereby granted, free of charge, to any person obtaining a copy of this
//! software and associated documentation files (the "Software"), to deal in the Software
//! without restriction, including without limitation the rights to use, copy, modify, merge,
//! publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons
//! to whom the Software is furnished to do so, subject to the following conditions:
//!
//! The above copyright notice and this permission notice shall be included in all copies or
//! substantial portions of the Software.
//!
//! THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED,
//! INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR
//! PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE
//! FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR
//! OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER
//! DEALINGS IN THE SOFTWARE.

use crate::state::State;
use smithay::reexports::wayland_protocols::xdg::foreign::zv2::server::{
    zxdg_exported_v2::{self, ZxdgExportedV2},
    zxdg_exporter_v2::{self, ZxdgExporterV2},
    zxdg_imported_v2::{self, ZxdgImportedV2},
    zxdg_importer_v2::{self, ZxdgImporterV2},
};
use smithay::reexports::wayland_server::backend::ClientId;
use smithay::reexports::wayland_server::protocol::wl_surface::WlSurface;
use smithay::reexports::wayland_server::{Client, DataInit, DisplayHandle, New, Resource};
use smithay::wayland::compositor::{get_role, with_states};
use smithay::wayland::shell::is_valid_parent;
use smithay::wayland::shell::xdg::{
    ToplevelSurface, XdgShellHandler, XdgToplevelSurfaceData, XDG_TOPLEVEL_ROLE,
};
use smithay::wayland::{Dispatch2, GlobalDispatch2};
use std::collections::HashMap;
use std::io::Read;

/// The exported toplevels, by handle.
#[derive(Default)]
pub struct Foreign {
    pub(crate) exports: HashMap<String, Export>,
}

pub(crate) struct Export {
    surface: WlSurface,
    /// The imported objects of the handle, each told when the handle is revoked.
    imports: Vec<ZxdgImportedV2>,
    /// The toplevels an importer made children of the surface, with the imported object that did.
    pub(crate) children: Vec<(WlSurface, ZxdgImportedV2)>,
}

/// The user data of the exporter global and of every exporter.
pub struct Exporter;
/// The user data of the importer global and of every importer.
pub struct Importer;
/// The user data of an exported object: its handle, or none when the compositor could not make one.
pub struct Exported(Option<String>);
/// The user data of an imported object: the handle it was made from.
pub struct Imported(String);

/// Offers both globals to every client.
pub fn global(dh: &DisplayHandle) {
    dh.create_global::<State, ZxdgExporterV2, _>(1, Exporter);
    dh.create_global::<State, ZxdgImporterV2, _>(1, Importer);
}

/// A new handle: 128 random bits as hexadecimal, so that a client cannot guess another's.
fn new_handle() -> std::io::Result<String> {
    let mut bits = [0u8; 16];
    std::fs::File::open("/dev/urandom")?.read_exact(&mut bits)?;
    Ok(format!("{:032x}", u128::from_be_bytes(bits)))
}

impl GlobalDispatch2<ZxdgExporterV2, State> for Exporter {
    fn bind(
        &self,
        _state: &mut State,
        _dh: &DisplayHandle,
        _client: &Client,
        resource: New<ZxdgExporterV2>,
        data_init: &mut DataInit<'_, State>,
    ) {
        data_init.init(resource, Exporter);
    }
}

impl Dispatch2<ZxdgExporterV2, State> for Exporter {
    fn request(
        &self,
        state: &mut State,
        _client: &Client,
        exporter: &ZxdgExporterV2,
        request: zxdg_exporter_v2::Request,
        _dh: &DisplayHandle,
        data_init: &mut DataInit<'_, State>,
    ) {
        match request {
            zxdg_exporter_v2::Request::ExportToplevel { id, surface } => {
                if get_role(&surface) != Some(XDG_TOPLEVEL_ROLE) {
                    exporter.post_error(
                        zxdg_exporter_v2::Error::InvalidSurface,
                        "the exported surface is not an xdg_toplevel",
                    );
                    return;
                }
                match new_handle() {
                    Ok(handle) => {
                        let exported = data_init.init(id, Exported(Some(handle.clone())));
                        exported.handle(handle.clone());
                        state.foreign.exports.insert(
                            handle,
                            Export {
                                surface,
                                imports: Vec::new(),
                                children: Vec::new(),
                            },
                        );
                    }
                    // An exported object with no handle: it exports nothing. The protocol has no
                    // event or error for a handle that could not be made (`invalid_surface` is
                    // for a surface that is no toplevel), so the client waits for `handle`; the
                    // failure is in the log only.
                    Err(error) => {
                        tracing::error!(%error, "a handle for an exported toplevel could not be made");
                        data_init.init(id, Exported(None));
                    }
                }
            }
            zxdg_exporter_v2::Request::Destroy => {}
            _ => {}
        }
    }
}

impl Dispatch2<ZxdgExportedV2, State> for Exported {
    fn request(
        &self,
        _state: &mut State,
        _client: &Client,
        _exported: &ZxdgExportedV2,
        _request: zxdg_exported_v2::Request,
        _dh: &DisplayHandle,
        _data_init: &mut DataInit<'_, State>,
    ) {
    }

    /// The exported object is destroyed, or its client is gone: the handle is revoked.
    fn destroyed(&self, state: &mut State, _client: ClientId, _exported: &ZxdgExportedV2) {
        if let Some(handle) = &self.0 {
            state.foreign_revoke(handle);
        }
    }
}

impl GlobalDispatch2<ZxdgImporterV2, State> for Importer {
    fn bind(
        &self,
        _state: &mut State,
        _dh: &DisplayHandle,
        _client: &Client,
        resource: New<ZxdgImporterV2>,
        data_init: &mut DataInit<'_, State>,
    ) {
        data_init.init(resource, Importer);
    }
}

impl Dispatch2<ZxdgImporterV2, State> for Importer {
    fn request(
        &self,
        state: &mut State,
        _client: &Client,
        _importer: &ZxdgImporterV2,
        request: zxdg_importer_v2::Request,
        _dh: &DisplayHandle,
        data_init: &mut DataInit<'_, State>,
    ) {
        match request {
            zxdg_importer_v2::Request::ImportToplevel { id, handle } => {
                let imported = data_init.init(id, Imported(handle.clone()));
                match state.foreign.exports.get_mut(&handle) {
                    Some(export) if export.surface.is_alive() => export.imports.push(imported),
                    // A handle that is not valid, or no longer: the object is born dead.
                    _ => imported.destroyed(),
                }
            }
            zxdg_importer_v2::Request::Destroy => {}
            _ => {}
        }
    }
}

impl Dispatch2<ZxdgImportedV2, State> for Imported {
    fn request(
        &self,
        state: &mut State,
        _client: &Client,
        imported: &ZxdgImportedV2,
        request: zxdg_imported_v2::Request,
        _dh: &DisplayHandle,
        _data_init: &mut DataInit<'_, State>,
    ) {
        let zxdg_imported_v2::Request::SetParentOf { surface: child } = request else {
            return;
        };
        // An object whose handle is revoked sets no parent.
        let Some(export) = state.foreign.exports.get(&self.0) else {
            return;
        };
        let parent = export.surface.clone();
        // `Some(true)` when the parent changed, `None` when the child is no toplevel.
        let changed = with_states(&child, |states| {
            let data = states.data_map.get::<XdgToplevelSurfaceData>()?;
            // A parent that would make a loop is refused, as `xdg_toplevel.set_parent` does.
            if !is_valid_parent(&child, &parent) {
                return None;
            }
            let mut role = data.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
            let changed = role.parent.as_ref() != Some(&parent);
            role.parent = Some(parent.clone());
            Some(changed)
        });
        let Some(changed) = changed else {
            imported.post_error(
                zxdg_imported_v2::Error::InvalidSurface,
                "the child is not an xdg_toplevel, or the parent would make a loop",
            );
            return;
        };
        // One parent at a time: the child leaves the other handles it was a child of.
        for export in state.foreign.exports.values_mut() {
            export.children.retain(|(kept, _)| *kept != child);
        }
        if let Some(export) = state.foreign.exports.get_mut(&self.0) {
            export.children.push((child.clone(), imported.clone()));
        }
        if changed {
            state.foreign_parent_changed(&child);
        }
    }

    /// The imported object is destroyed, or its client is gone: the toplevels it made children
    /// stop being so.
    fn destroyed(&self, state: &mut State, _client: ClientId, imported: &ZxdgImportedV2) {
        let Some(export) = state.foreign.exports.get_mut(&self.0) else {
            return;
        };
        export.imports.retain(|kept| kept != imported);
        let parent = export.surface.clone();
        let (gone, kept): (Vec<_>, Vec<_>) = export
            .children
            .drain(..)
            .partition(|(_, by)| by == imported);
        export.children = kept;
        for (child, _) in gone {
            state.foreign_unparent(&child, &parent);
        }
    }
}

impl State {
    /// The handle is no longer valid: its children lose their parent and every imported object
    /// is told.
    fn foreign_revoke(&mut self, handle: &str) {
        let Some(export) = self.foreign.exports.remove(handle) else {
            return;
        };
        for (child, _) in &export.children {
            self.foreign_unparent(child, &export.surface);
        }
        for imported in &export.imports {
            imported.destroyed();
        }
    }

    /// A toplevel was destroyed: the handles it was exported under are revoked, whether or not
    /// the exported objects are.
    pub(crate) fn foreign_toplevel_destroyed(&mut self, surface: &WlSurface) {
        // A child that is gone is no child of any handle, whoever its parent is.
        for export in self.foreign.exports.values_mut() {
            export.children.retain(|(child, _)| child != surface);
        }
        let handles: Vec<String> = self
            .foreign
            .exports
            .iter()
            .filter(|(_, export)| export.surface == *surface)
            .map(|(handle, _)| handle.clone())
            .collect();
        for handle in handles {
            self.foreign_revoke(&handle);
        }
    }

    /// `child` stops being a child of `parent`, if it still is.
    fn foreign_unparent(&mut self, child: &WlSurface, parent: &WlSurface) {
        let changed = with_states(child, |states| {
            let Some(data) = states.data_map.get::<XdgToplevelSurfaceData>() else {
                return false;
            };
            let mut role = data.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
            let is_child = role.parent.as_ref() == Some(parent);
            if is_child {
                role.parent = None;
            }
            is_child
        });
        if changed {
            self.foreign_parent_changed(child);
        }
    }

    /// `toplevel` has a parent that is not the one an imported object gave it: the client set it
    /// with `xdg_toplevel.set_parent`, which wins, so the toplevel is no child of any handle.
    /// Without this, destroying the imported object would clear a parent the client chose.
    /// (`set_parent` to the very parent the import gave is no change, and Smithay does not report
    /// it.)
    pub(crate) fn foreign_parent_set_natively(&mut self, toplevel: &ToplevelSurface) {
        let parent = toplevel.parent();
        let child = toplevel.wl_surface();
        for export in self.foreign.exports.values_mut() {
            if parent.as_ref() != Some(&export.surface) {
                export.children.retain(|(kept, _)| kept != child);
            }
        }
    }

    fn foreign_parent_changed(&mut self, child: &WlSurface) {
        let toplevel = self
            .xdg_shell_state
            .toplevel_surfaces()
            .iter()
            .find(|toplevel| toplevel.wl_surface() == child)
            .cloned();
        if let Some(toplevel) = toplevel {
            XdgShellHandler::parent_changed(self, toplevel);
        }
    }
}
