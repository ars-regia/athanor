# accesskit_atspi_common 0.21.0, patched

The published crate `accesskit_atspi_common` 0.21.0 (crates.io, `MIT OR Apache-2.0`, sha256
`52c182f9c282ac9c5638d876d551d15e5f7d397ec263349a0c6a2b61595dd5e4`, AccessKit commit
`ce8164ba92995cfa86005b6259115e08c8244253`, `adapters/atspi-common`), with two changes. The
licence files come from the AccessKit repository at that commit, as the published crate carries
none. `[patch.crates-io]` in the repository's root `Cargo.toml` and in
`system/athanor-sal/gallery/Cargo.toml` puts it in place of the published crate.

## 1. The expanded and has-popup states

### Why

The AT-SPI bridge maps neither a node's `expanded` nor its `has_popup` to an AT-SPI state, so
Orca cannot say "collapsed button" for the dock's menu buttons, nor "opens popup" for them and
for a text field with its edit menu, as it does for GTK's (Athanor #638, #719;
`doc_own_stack.md` OS7 and OS8). Neither 0.21.1 nor AccessKit's main of 2026-11-03 (`52d3ee49`)
maps them.

### The change

`src/node.rs`, at the end of `NodeWrapper::state`, as W3C Core-AAM 1.2 maps `aria-expanded`
(`STATE_EXPANDABLE`, and `STATE_EXPANDED` when true) and `aria-haspopup` (`STATE_HAS_POPUP`), and
as GTK 4 exposes its menu buttons: no `STATE_COLLAPSED`, which Orca does not need to say
"collapsed". The state-change events follow without another change, as `notify_state_changes`
compares the old and new state sets.

```diff
--- a/src/node.rs
+++ b/src/node.rs
@@ -385,6 +385,16 @@
             atspi_state.insert(State::Focused);
         }
 
+        if let Some(expanded) = state.data().is_expanded() {
+            atspi_state.insert(State::Expandable);
+            if expanded {
+                atspi_state.insert(State::Expanded);
+            }
+        }
+        if state.has_popup().is_some() {
+            atspi_state.insert(State::HasPopup);
+        }
+
         atspi_state
     }
```

`athanor-sal`'s test `a11y::tests::a_menu_button_and_a_field_reach_at_spi_with_their_popup_states`
fails without it.

### Upstream

- AccessKit#824: a combo box's value, and the expanded and collapsed states, do not reach screen
  readers (open).
- AccessKit#665: adds the expandable, expanded and collapsed states (open, not merged); this
  patch leaves out the collapsed state, which Core-AAM does not map, and adds has-popup.
- Athanor's proposal of this change, has-popup included, is a comment on AccessKit#824.

## 2. The focus before the window activation

### Why

When a window takes the keyboard back, from a dialog that closed, GTK 4.20 reports
`state-changed:focused` on the widget that takes the focus before `state-changed:active` and
`window:activate` on its window. With that order Orca says the window's name and role ("Galleria
cornice" in Italian); with the bridge's, the activation first, it says only the name (Athanor
#1123). `accesskit_consumer` reports a tree's focus only while its host is focused, so the
application cannot order the two itself: the bridge emits both from one call of
`AdapterChangeHandler::focus_moved`. Neither 0.21.1 nor AccessKit's main of 2026-11-09
(`dea4384134abe5d7b0734f56a0fdd8b80a16dfdd`) changes that order.

### The change

`src/adapter.rs`, `AdapterChangeHandler::focus_moved`: the new focus, and its text selection, are
emitted before the window is activated. A window deactivated still is before the old focus is
cleared.

```diff
--- a/src/adapter.rs
+++ b/src/adapter.rs
@@ -322,6 +322,11 @@
     }
 
     fn focus_moved(&mut self, old_node: Option<&NodeRef>, new_node: Option<&NodeRef>) {
+        if let Some(node) = new_node {
+            self.adapter
+                .emit_object_event(node.id(), ObjectEvent::StateChanged(State::Focused, true));
+            self.emit_text_selection_change(None, node);
+        }
         if let (None, Some(new_node)) = (old_node, new_node) {
             if let Some(root_window) = root_window(new_node.tree_state) {
                 self.adapter.window_activated(&NodeWrapper(&root_window));
@@ -331,11 +336,6 @@
                 self.adapter.window_deactivated(&NodeWrapper(&root_window));
             }
         }
-        if let Some(node) = new_node {
-            self.adapter
-                .emit_object_event(node.id(), ObjectEvent::StateChanged(State::Focused, true));
-            self.emit_text_selection_change(None, node);
-        }
         if let Some(node) = old_node {
             self.adapter
                 .emit_object_event(node.id(), ObjectEvent::StateChanged(State::Focused, false));
```

`athanor-sal`'s test `a11y::tests::a_window_taking_the_keyboard_back_reports_the_focus_before_its_activation`
fails without it.

### Upstream

No AccessKit issue or pull request covers this order as of 2026-11-13; proposing it is the
maintainer's decision.

## When it goes

With the first release of `accesskit_atspi_common` that maps both the expanded state and
has-popup and emits the focus before the window activation: remove this directory, both `[patch.crates-io]` entries, the root workspace's
`exclude` entry and `GALLERY_PATCH` in `scripts/verify.py`, and update the lockfiles with cargo.
