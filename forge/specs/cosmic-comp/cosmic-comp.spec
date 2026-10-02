# Athanor's cosmic-comp: Fedora's package with one focus patch, same Name so that it replaces
# Fedora's build instead of sitting beside it (like forge/specs/just). The release spells out
# Fedora's, so that 1.8.0-1.fc43.athanor1 sorts above 1.8.0-1.fc43 and below 1.8.0-2.fc43: the
# Nix builder defines no %dist.
%global fedora_release 1.fc43
# The commit of upstream's tag epoch-%{version}: build.rs embeds it as GIT_HASH.
%global commit a55785993e8ef6aad38862cb1a9e1ccaad3c340d
%global cosmic_minver 1.8.0

Name:           cosmic-comp
Version:        1.8.0
Release:        %{fedora_release}.athanor1
Summary:        Wayland compositor of the COSMIC desktop, with Athanor's focus fix

# Fedora's full expression: the binary statically links the crates. GPL-3.0-only is cosmic-comp's own.
License:        (0BSD OR Apache-2.0 OR MIT) AND (Apache-2.0 OR Apache-2.0 WITH LLVM-exception OR MIT) AND (Apache-2.0 OR BSD-2-Clause OR MIT) AND (Apache-2.0 OR BSD-3-Clause OR MIT) AND (Apache-2.0 OR BSD-3-Clause) AND (Apache-2.0 OR CC0-1.0 OR MIT) AND (Apache-2.0 OR GPL-2.0-only) AND (Apache-2.0 OR LGPL-2.1-or-later OR MIT) AND (Apache-2.0 OR MIT OR Unlicense) AND (Apache-2.0 OR MIT OR Zlib) AND (Apache-2.0 OR MIT) AND (LGPL-3.0-or-later OR MIT) AND (MIT OR Unlicense) AND Apache-2.0 AND BSD-2-Clause AND BSD-3-Clause AND BSL-1.0 AND CC0-1.0 AND GPL-3.0-only AND ISC AND MIT AND MPL-2.0 AND Unicode-3.0 AND Zlib
URL:            https://github.com/pop-os/cosmic-comp
Source0:        https://github.com/pop-os/cosmic-comp/archive/epoch-%{version}/cosmic-comp-%{version}.tar.gz
# Submitted upstream; drop it when a release carries it.
Patch0:         0001-shell-focus-Reconcile-focus-when-a-layer-surface-cha.patch

BuildRequires:  cargo rust

Requires:       libseat%{?_isa}
Requires:       libwayland-server%{?_isa}
Requires:       xorg-x11-server-Xwayland
Requires:       cosmic-icon-theme >= %{cosmic_minver}

%description
The compositor of the COSMIC desktop, built from the upstream release archive with the crates
pinned by its Cargo.lock, plus a patch that reconciles the keyboard focus when a layer surface
changes its keyboard interactivity. Athanor's launcher needs it.

%prep
%autosetup -p1 -n cosmic-comp-epoch-%{version}

%build
%set_build_flags
# build.rs embeds the output of `git rev-parse HEAD` and ignores GIT_HASH: inside an unrelated
# checkout it would record that checkout's commit, outside one an empty string. A stub git
# that prints the tag's commit makes the build independent of where it runs.
mkdir -p .athanor-bin
printf '#!/bin/sh\necho %{commit}\n' > .athanor-bin/git
chmod +x .athanor-bin/git
export PATH="$PWD/.athanor-bin:$PATH"
# The release profile of Cargo.toml (fat LTO), not Fedora's rpm profile: the builder has no
# cargo-rpm-macros (%cargo_build), and rpmbuild runs with --nodeps.
cargo build --release --locked

%install
make install DESTDIR=%{buildroot} prefix=%{_prefix}

%files
%license LICENSE
%{_bindir}/cosmic-comp
%{_datadir}/cosmic/com.system76.CosmicSettings.Shortcuts/v1/defaults
%{_datadir}/cosmic/com.system76.CosmicSettings.WindowRules/v1/tiling_exception_defaults

%changelog
* Fri Oct 02 2026 Athanor Forge <forge@athanor.os> - 1.8.0-1.fc43.athanor1
- Fedora's cosmic-comp 1.8.0-1.fc43 built from the upstream archive, with the layer-surface
  focus patch.
