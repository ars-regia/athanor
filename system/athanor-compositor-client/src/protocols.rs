//! Client bindings for the COSMIC protocols, generated at build time by `wayland-scanner`
//! from the descriptions vendored under `protocols/`. The descriptions carry permissive
//! licences; `protocols/README.md` names the pinned upstream commit and each licence.

/// The module layout of `wayland-protocols`: `client` holds the generated types, and
/// `$deps` are the modules whose interfaces the description references.
macro_rules! generated {
    ($path:literal, [$($deps:path),*]) => {
        #[allow(
            dead_code,
            missing_docs,
            non_camel_case_types,
            non_snake_case,
            non_upper_case_globals,
            unused_imports,
            unused_variables,
            clippy::all
        )]
        pub mod client {
            use wayland_client;
            use wayland_client::protocol::*;
            $(use $deps::{client::*};)*

            pub mod __interfaces {
                // The generated tables name `wayland_backend`, which wayland-client re-exports.
                use wayland_client::backend as wayland_backend;
                use wayland_client::protocol::__interfaces::*;
                $(use $deps::{client::__interfaces::*};)*
                wayland_scanner::generate_interfaces!($path);
            }
            use self::__interfaces::*;

            wayland_scanner::generate_client_code!($path);
        }
    };
}

/// Only referenced by the toplevel protocols; cosmic-comp 1.8 offers version 2 instead.
pub mod workspace_v1 {
    generated!("protocols/cosmic-workspace-unstable-v1.xml", []);
}

pub mod workspace_v2 {
    generated!(
        "protocols/cosmic-workspace-unstable-v2.xml",
        [wayland_protocols::ext::workspace::v1]
    );
}

pub mod toplevel_info {
    generated!(
        "protocols/cosmic-toplevel-info-unstable-v1.xml",
        [
            crate::protocols::workspace_v1,
            wayland_protocols::ext::foreign_toplevel_list::v1,
            wayland_protocols::ext::workspace::v1
        ]
    );
}

pub mod toplevel_management {
    generated!(
        "protocols/cosmic-toplevel-management-unstable-v1.xml",
        [
            crate::protocols::toplevel_info,
            crate::protocols::workspace_v1,
            wayland_protocols::ext::workspace::v1
        ]
    );
}

pub mod keyboard_layout {
    generated!("protocols/cosmic-keyboard-layout-unstable-v1.xml", []);
}

pub mod a11y {
    generated!("protocols/cosmic-a11y-unstable-v1.xml", []);
}
