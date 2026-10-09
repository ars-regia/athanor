# Athanor's cosmic-comp: the upstream release at Fedora's version, built from upstream's
# archive with two patches of ours; Fedora's own downstream patches are not carried, and
# License and Requires were copied from Fedora's spec, so a bump diffs them by hand. Same
# Name so that it replaces Fedora's build instead of sitting beside it (like
# forge/specs/just). The release spells out Fedora's, so that 1.8.0-1.fc43.athanor1 sorts
# above 1.8.0-1.fc43 and below 1.8.0-2.fc43: the Nix builder defines no %%dist.
%global fedora_release 2.fc43
# The commit of upstream's tag epoch-%{version}, embedded as GIT_HASH.
%global commit 41497b42d9744d9963c4c6d17e8add49489b45ee
%global cosmic_minver 1.8.0

Name:           cosmic-comp
Version:        1.10.0
Release:        %{fedora_release}.athanor1
Summary:        Wayland compositor of the COSMIC desktop, with Athanor's focus fix

# Fedora's full expression: the binary statically links the crates. GPL-3.0-only is cosmic-comp's own.
License:        (0BSD OR Apache-2.0 OR MIT) AND (Apache-2.0 OR Apache-2.0 WITH LLVM-exception OR MIT) AND (Apache-2.0 OR BSD-2-Clause OR MIT) AND (Apache-2.0 OR BSD-3-Clause OR MIT) AND (Apache-2.0 OR BSD-3-Clause) AND (Apache-2.0 OR CC0-1.0 OR MIT) AND (Apache-2.0 OR GPL-2.0-only) AND (Apache-2.0 OR LGPL-2.1-or-later OR MIT) AND (Apache-2.0 OR MIT OR Unlicense) AND (Apache-2.0 OR MIT OR Zlib) AND (Apache-2.0 OR MIT) AND (LGPL-3.0-or-later OR MIT) AND (MIT OR Unlicense) AND Apache-2.0 AND BSD-2-Clause AND BSD-3-Clause AND BSL-1.0 AND CC0-1.0 AND GPL-3.0-only AND ISC AND MIT AND MPL-2.0 AND Unicode-3.0 AND Zlib
URL:            https://github.com/pop-os/cosmic-comp
Source0:        https://github.com/pop-os/cosmic-comp/archive/epoch-%{version}/cosmic-comp-%{version}.tar.gz
# Both submitted upstream; drop each when a release carries it.
Patch0:         0001-shell-focus-Reconcile-focus-when-a-layer-surface-cha.patch
Patch1:         0002-build-Take-the-commit-hash-from-GIT_HASH-when-it-is-.patch

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
# Patch1 makes build.rs record this commit instead of asking git, which a source archive lacks.
export GIT_HASH=%{commit}
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
* Fri Oct 09 2026 Athanor Forge <forge@athanor.os> - 1.10.0-2.fc43.athanor1
- Fedora's cosmic-comp 1.10.0-2.fc43.

* Thu Oct 08 2026 Athanor Forge <forge@athanor.os> - 1.9.0-1.fc43.athanor1
- Fedora's cosmic-comp 1.9.0-1.fc43; both patches still apply.

* Fri Oct 02 2026 Athanor Forge <forge@athanor.os> - 1.8.0-1.fc43.athanor1
- Fedora's cosmic-comp 1.8.0-1.fc43 built from the upstream archive, with the layer-surface
  focus patch.
