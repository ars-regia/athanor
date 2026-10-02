//! The tiling switch (doc_bar.md, BR3). It acts on the active workspace of the output its
//! surface is on.

pub use athanor_compositor_client::Tiling;

pub struct WorkspaceView<K> {
    pub id: K,
    pub active: bool,
    pub output: Option<String>,
    /// `None` until the compositor reports it: the switch is hidden.
    pub tiling: Option<Tiling>,
}

/// The active workspace on `connector`. A compositor that reports no outputs leaves one
/// case unambiguous: a single active workspace.
pub fn active_on<'a, K>(
    workspaces: &'a [WorkspaceView<K>],
    connector: Option<&str>,
) -> Option<&'a WorkspaceView<K>> {
    let mut active = workspaces.iter().filter(|view| view.active);
    if let Some(connector) = connector {
        if let Some(view) = active
            .clone()
            .find(|view| view.output.as_deref() == Some(connector))
        {
            return Some(view);
        }
        if workspaces.iter().any(|view| view.output.is_some()) {
            return None;
        }
    }
    let first = active.next()?;
    active.next().is_none().then_some(first)
}

pub fn toggled(tiling: Tiling) -> Tiling {
    match tiling {
        Tiling::Tiled => Tiling::Floating,
        Tiling::Floating => Tiling::Tiled,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn view(
        id: u8,
        active: bool,
        output: Option<&str>,
        tiling: Option<Tiling>,
    ) -> WorkspaceView<u8> {
        WorkspaceView {
            id,
            active,
            output: output.map(str::to_string),
            tiling,
        }
    }

    #[test]
    fn the_active_workspace_of_the_output() {
        let all = [
            view(1, false, Some("DP-1"), Some(Tiling::Tiled)),
            view(2, true, Some("DP-1"), Some(Tiling::Floating)),
            view(3, true, Some("HDMI-A-1"), Some(Tiling::Tiled)),
        ];
        assert_eq!(active_on(&all, Some("DP-1")).map(|view| view.id), Some(2));
        assert_eq!(
            active_on(&all, Some("HDMI-A-1")).map(|view| view.id),
            Some(3)
        );
        assert_eq!(active_on(&all, Some("eDP-1")).map(|view| view.id), None);
    }

    #[test]
    fn without_outputs_only_a_single_active_workspace_is_unambiguous() {
        let one = [
            view(1, true, None, Some(Tiling::Tiled)),
            view(2, false, None, None),
        ];
        assert_eq!(active_on(&one, Some("DP-1")).map(|view| view.id), Some(1));
        assert_eq!(active_on(&one, None).map(|view| view.id), Some(1));
        let two = [view(1, true, None, None), view(2, true, None, None)];
        assert_eq!(active_on(&two, Some("DP-1")).map(|view| view.id), None);
    }

    #[test]
    fn toggling_swaps_the_mode() {
        assert_eq!(toggled(Tiling::Tiled), Tiling::Floating);
        assert_eq!(toggled(Tiling::Floating), Tiling::Tiled);
    }
}
