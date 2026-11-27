%global debug_package %{nil}
Name:           athanor-polkit-agent
Version:        1.0.0
Release:        1%{?athanor_content}%{?dist}
Summary:        The Athanor polkit authentication agent
License:        GPL-3.0-or-later

BuildRequires:  rust cargo gcc

%description
The polkit authentication agent of doc_lock_and_prompts.md (LP11), written in Athanor's own
toolkit, Sal, and run as a guard of the session. This release is LP18 step 5 in progress: the
request queue, the identity choice and the description of the requesting program, with no bus
and no screen yet; the program registers with nobody and exits, which it says. The unit is
installed and not enabled; cosmic-osd's retirement adds its preset (OD17 step 5).

%prep

%build
%set_build_flags
cargo build --release --locked -p %{name}

%install
install -D -m 0755 target/release/athanor-polkit-agent %{buildroot}/usr/bin/athanor-polkit-agent
install -D -m 0644 forge/specs/athanor-polkit-agent/athanor-polkit-agent-1.0.0/data/athanor-polkit-agent.service \
    %{buildroot}/usr/lib/systemd/user/athanor-polkit-agent.service

%files
/usr/bin/athanor-polkit-agent
/usr/lib/systemd/user/athanor-polkit-agent.service

%changelog
* Fri Nov 27 2026 Athanor Forge <forge@athanor.os> - 1.0.0-1
- First release (doc_lock_and_prompts.md, LP18 step 5, P2): the crate's toolkit-free core, the
  guard user unit confined as D43 asks, and the package. The program is not an agent yet and
  exits; no preset enables the unit.
