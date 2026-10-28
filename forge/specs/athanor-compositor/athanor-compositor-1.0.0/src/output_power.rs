//! wlr-output-power-management (doc_own_stack.md decision 28; the output power plan): the idle
//! daemon turns the outputs off and on. It is privileged (OS4, D1): a client that holds it can
//! blank the session. The pinned Smithay has no server for it, so the compositor serves it here.
//!
//! One client controls an output at a time (D2): another client's object for it is sent `failed`
//! and stays inert. When the controlling client's last object goes, by `destroy` or with the
//! client, an output it left off turns on (D3), so a daemon that dies never leaves the screens
//! dark.

use crate::identity::Privilege;
use crate::state::{granted, State};
use smithay::output::Output;
use smithay::reexports::wayland_protocols_wlr::output_power_management::v1::server::{
    zwlr_output_power_manager_v1::{self, ZwlrOutputPowerManagerV1},
    zwlr_output_power_v1::{self, Mode, ZwlrOutputPowerV1},
};
use smithay::reexports::wayland_server::backend::ClientId;
use smithay::reexports::wayland_server::{Client, DataInit, DisplayHandle, New, Resource, WEnum};
use smithay::wayland::{Dispatch2, GlobalDispatch2};

/// The version offered, the protocol's only one.
const VERSION: u32 = 1;

/// The user data of the global and of every object of the protocol.
pub struct Power;

/// The outputs that are off, and the objects that control the outputs.
#[derive(Default)]
pub struct OutputPower {
    off: Vec<Output>,
    controls: Vec<Control>,
}

/// A live object of the client that controls its output. An object refused with `failed` is
/// not one.
struct Control {
    object: ZwlrOutputPowerV1,
    output: Output,
    client: ClientId,
}

impl OutputPower {
    /// Whether `output` is on.
    pub fn is_on(&self, output: &Output) -> bool {
        !self.off.contains(output)
    }

    /// The client that controls `output`, if one does.
    fn controller(&self, output: &Output) -> Option<&ClientId> {
        self.controls
            .iter()
            .find(|control| &control.output == output)
            .map(|control| &control.client)
    }
}

/// Offers the global to the clients the allow-list grants it.
pub fn global(dh: &DisplayHandle) {
    dh.create_global::<State, ZwlrOutputPowerManagerV1, _>(VERSION, Power);
}

impl GlobalDispatch2<ZwlrOutputPowerManagerV1, State> for Power {
    fn bind(
        &self,
        _state: &mut State,
        _dh: &DisplayHandle,
        _client: &Client,
        resource: New<ZwlrOutputPowerManagerV1>,
        data_init: &mut DataInit<'_, State>,
    ) {
        data_init.init(resource, Power);
    }

    fn can_view(&self, client: &Client) -> bool {
        granted(client, Privilege::OutputPower)
    }
}

impl Dispatch2<ZwlrOutputPowerManagerV1, State> for Power {
    fn request(
        &self,
        state: &mut State,
        client: &Client,
        _manager: &ZwlrOutputPowerManagerV1,
        request: zwlr_output_power_manager_v1::Request,
        _dh: &DisplayHandle,
        data_init: &mut DataInit<'_, State>,
    ) {
        let zwlr_output_power_manager_v1::Request::GetOutputPower { id, output } = request else {
            return;
        };
        let object = data_init.init(id, Power);
        let power = &mut state.output_power;
        let Some(output) = Output::from_resource(&output) else {
            object.failed();
            return;
        };
        if power
            .controller(&output)
            .is_some_and(|controller| *controller != client.id())
        {
            tracing::info!(
                output = output.name(),
                "another client controls the output's power: refused"
            );
            object.failed();
            return;
        }
        object.mode(mode(power.is_on(&output)));
        power.controls.push(Control {
            object,
            output,
            client: client.id(),
        });
    }
}

impl Dispatch2<ZwlrOutputPowerV1, State> for Power {
    fn request(
        &self,
        state: &mut State,
        _client: &Client,
        object: &ZwlrOutputPowerV1,
        request: zwlr_output_power_v1::Request,
        _dh: &DisplayHandle,
        _data_init: &mut DataInit<'_, State>,
    ) {
        let zwlr_output_power_v1::Request::SetMode { mode } = request else {
            return;
        };
        let on = match mode {
            WEnum::Value(Mode::On) => true,
            WEnum::Value(Mode::Off) => false,
            _ => {
                object.post_error(
                    zwlr_output_power_v1::Error::InvalidMode,
                    "no such power mode",
                );
                return;
            }
        };
        let output = state
            .output_power
            .controls
            .iter()
            .find(|control| &control.object == object)
            .map(|control| control.output.clone());
        // A refused object is inert.
        if let Some(output) = output {
            state.set_output_power(&output, on);
        }
    }

    fn destroyed(&self, state: &mut State, _client: ClientId, object: &ZwlrOutputPowerV1) {
        let power = &mut state.output_power;
        let Some(at) = power
            .controls
            .iter()
            .position(|control| &control.object == object)
        else {
            return;
        };
        let output = power.controls.remove(at).output;
        if power.controller(&output).is_none() && !power.is_on(&output) {
            tracing::info!(
                output = output.name(),
                "the power client let go of the output: it turns on"
            );
            state.set_output_power(&output, true);
        }
    }
}

fn mode(on: bool) -> Mode {
    if on {
        Mode::On
    } else {
        Mode::Off
    }
}

impl State {
    /// Turns `output` on or off, and tells every object that controls it. Nothing happens when
    /// it already is.
    pub fn set_output_power(&mut self, output: &Output, on: bool) {
        let power = &mut self.output_power;
        if power.is_on(output) == on {
            return;
        }
        if on {
            power.off.retain(|off| off != output);
        } else {
            power.off.push(output.clone());
        }
        for control in power.controls.iter().filter(|c| &c.output == output) {
            control.object.mode(mode(on));
        }
    }

    /// An output that goes: the objects that controlled it are sent `failed`.
    pub fn forget_output_power(&mut self, output: &Output) {
        let power = &mut self.output_power;
        power.off.retain(|off| off != output);
        power.controls.retain(|control| {
            let gone = &control.output == output;
            if gone {
                control.object.failed();
            }
            !gone
        });
    }
}
