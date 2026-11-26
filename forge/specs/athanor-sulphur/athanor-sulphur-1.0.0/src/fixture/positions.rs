//! Each output's place in the layout reaches its clients through `xdg_output`, so a client that
//! composes the outputs, as the portal's screenshot does (`doc_portal.md` PT8), finds them where
//! the space has them rather than all at the origin.

use super::*;
use wayland_protocols::xdg::xdg_output::zv1::client::{zxdg_output_manager_v1, zxdg_output_v1};

/// The last `logical_position` an `xdg_output` was sent.
type Position = Arc<Mutex<Option<(i32, i32)>>>;

delegate_noop!(Client: zxdg_output_manager_v1::ZxdgOutputManagerV1);

impl Dispatch<zxdg_output_v1::ZxdgOutputV1, Position> for Client {
    fn event(
        _: &mut Self,
        _: &zxdg_output_v1::ZxdgOutputV1,
        event: zxdg_output_v1::Event,
        position: &Position,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let zxdg_output_v1::Event::LogicalPosition { x, y } = event {
            *position.lock().expect("position") = Some((x, y));
        }
    }
}

/// The positions the client was last sent, sorted, once every output has one.
fn sent(positions: &[Position]) -> Option<Vec<(i32, i32)>> {
    let mut sent: Vec<(i32, i32)> = positions
        .iter()
        .map(|position| *position.lock().expect("position"))
        .collect::<Option<_>>()?;
    sent.sort_unstable();
    Some(sent)
}

/// The outputs' positions in the space, sorted.
fn placed(run: &Running) -> Vec<(i32, i32)> {
    let mut placed: Vec<(i32, i32)> = run
        .state
        .space
        .outputs()
        .filter_map(|output| run.state.space.output_geometry(output))
        .map(|area| (area.loc.x, area.loc.y))
        .collect();
    placed.sort_unstable();
    placed
}

/// The outputs laid side by side reach the client at their places in the space, and an output
/// placed again later reaches it at the new place.
#[test]
fn each_output_reaches_its_clients_at_its_place_in_the_layout() {
    let mut run = Running::start(None);
    let mut local = Local::connect(&mut run, None);
    let qh = local.queue.handle();
    let manager: zxdg_output_manager_v1::ZxdgOutputManagerV1 = local.bind(3);
    let positions: Vec<Position> = local
        .client
        .globals
        .iter()
        .filter(|(_, interface, _)| interface == "wl_output")
        .map(|(name, _, version)| {
            let output: wl_output::WlOutput =
                local.registry.bind(*name, (*version).min(4), &qh, ());
            let position = Position::default();
            manager.get_xdg_output(&output, &qh, position.clone());
            position
        })
        .collect();
    assert_eq!(positions.len(), OUTPUTS.len());
    local.pump(&mut run, "every output has a position", |_, _| {
        sent(&positions).is_some()
    });
    let side_by_side = placed(&run);
    assert_eq!(
        side_by_side,
        [(0, 0), (OUTPUTS[0].width, 0)],
        "side by side"
    );
    assert_eq!(sent(&positions).as_deref(), Some(side_by_side.as_slice()));

    let second = output_at(&run, 1);
    run.state.place_output(&second, (0, 800).into());
    let below = placed(&run);
    assert_eq!(below, [(0, 0), (0, 800)]);
    local.pump(&mut run, "the new position is sent", |_, _| {
        sent(&positions).as_deref() == Some(below.as_slice())
    });
    run.stop();
}
