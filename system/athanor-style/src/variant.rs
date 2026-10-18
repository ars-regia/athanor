//! The four Calmo variants and their GTK stylesheets, as `calmo/generate.py` writes them.
//! Kept apart from `calmo`'s loader, which needs GTK, so that the appearance builds without it.

/// One of the four variants the contrast gate validates.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Variant {
    Light,
    Dark,
    LightHc,
    DarkHc,
}

impl Variant {
    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            "light" => Some(Self::Light),
            "dark" => Some(Self::Dark),
            "light-hc" => Some(Self::LightHc),
            "dark-hc" => Some(Self::DarkHc),
            _ => None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Light => "light",
            Self::Dark => "dark",
            Self::LightHc => "light-hc",
            Self::DarkHc => "dark-hc",
        }
    }

    pub fn is_dark(self) -> bool {
        matches!(self, Self::Dark | Self::DarkHc)
    }

    pub fn is_high_contrast(self) -> bool {
        matches!(self, Self::LightHc | Self::DarkHc)
    }

    pub fn with_high_contrast(self, on: bool) -> Self {
        match (self.is_dark(), on) {
            (false, false) => Self::Light,
            (false, true) => Self::LightHc,
            (true, false) => Self::Dark,
            (true, true) => Self::DarkHc,
        }
    }

    pub fn css(self) -> &'static str {
        match self {
            Self::Light => include_str!("../calmo/generated/css/calmo-light.css"),
            Self::Dark => include_str!("../calmo/generated/css/calmo-dark.css"),
            Self::LightHc => include_str!("../calmo/generated/css/calmo-light-hc.css"),
            Self::DarkHc => include_str!("../calmo/generated/css/calmo-dark-hc.css"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALL: [Variant; 4] = [
        Variant::Light,
        Variant::Dark,
        Variant::LightHc,
        Variant::DarkHc,
    ];

    #[test]
    fn names_round_trip() {
        for variant in ALL {
            assert_eq!(Variant::from_name(variant.name()), Some(variant));
        }
        assert_eq!(Variant::from_name("sepia"), None);
    }

    #[test]
    fn high_contrast_keeps_the_mode() {
        assert_eq!(Variant::Light.with_high_contrast(true), Variant::LightHc);
        assert_eq!(Variant::DarkHc.with_high_contrast(false), Variant::Dark);
        assert!(Variant::DarkHc.is_dark() && Variant::DarkHc.is_high_contrast());
        assert!(!Variant::Light.is_dark() && !Variant::Light.is_high_contrast());
    }

    #[test]
    fn every_variant_embeds_its_own_generated_sheet() {
        for variant in ALL {
            let css = variant.css();
            assert!(
                css.contains(&format!("/* Variant: {} */", variant.name())),
                "{variant:?}"
            );
            assert!(css.contains("window.athanor-greeter"));
        }
        assert!(Variant::Light
            .css()
            .contains("@define-color ath_acc #2e44c2;"));
        assert!(Variant::Dark
            .css()
            .contains("@define-color ath_acc #8898f7;"));
    }
}
