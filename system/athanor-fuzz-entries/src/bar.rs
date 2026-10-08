//! GVariants any process on the session bus can send to the bar. The bytes are the serialised
//! form of the type the bar expects; GLib reads a malformed serialisation as default values,
//! so every input reaches the parser.

use athanor_bar::{dbusmenu, notices, tray};
use glib::{Variant, VariantTy};

use crate::Setup;

fn variant(data: &[u8], signature: &str) -> std::io::Result<Variant> {
    let ty = VariantTy::new(signature).map_err(std::io::Error::other)?;
    Ok(Variant::from_data_with_type(Vec::from(data), ty))
}

/// `tray::read` over an `a{sv}`; the first byte is the pixel size the button draws at.
pub fn tray(data: &[u8]) -> Setup {
    if let Some((&px, rest)) = data.split_first() {
        let _ = tray::read(&variant(rest, "a{sv}")?, u32::from(px));
    }
    Ok(())
}

/// `dbusmenu::parse_layout` over a `(u(ia{sv}av))`.
pub fn dbusmenu(data: &[u8]) -> Setup {
    let _ = dbusmenu::parse_layout(&variant(data, dbusmenu::LAYOUT_SIGNATURE)?);
    Ok(())
}

/// `Notice::decode` over the wire tuple of the private interface.
pub fn notice(data: &[u8]) -> Setup {
    let _ = notices::Notice::decode(&variant(data, notices::WIRE_SIGNATURE)?);
    Ok(())
}

/// `png_within`; the first four bytes are the side bound.
pub fn png(data: &[u8]) -> Setup {
    if let Some((bound, rest)) = data.split_first_chunk::<4>() {
        let _ = notices::png_within(rest, u32::from_le_bytes(*bound));
    }
    Ok(())
}
