Name:           athanor-keylime
Version:        1.0
Release:        5%{?dist}
Summary:        Athanor OS Keylime Agent Configuration
License:        GPL-3.0-or-later
URL:            https://github.com/ars-regia/athanor
Source0:        99-athanor.conf

Requires:       keylime-agent
Requires:       tpm2-tools
BuildArch:      noarch

%description
Configuration package for the Keylime agent in Athanor OS: a drop-in that sets the
agent's uuid option. It does not seal or bind anything.

%prep
# Nothing to unpack: the drop-in is Source0.

%build

%install
install -D -m 0644 %{SOURCE0} %{buildroot}/etc/keylime/agent.conf.d/99-athanor.conf

%files
# The drop-in directory belongs to keylime-agent-rust-common, which keylime-agent
# pulls in; owning it here with other attributes is an RPM file conflict.
%config(noreplace) /etc/keylime/agent.conf.d/99-athanor.conf

%changelog
* Tue Oct 06 2026 Athanor Forge <forge@athanor.os> - 1.0-5
- Drop measured_boot_imports from the agent drop-in: it is a Keylime verifier
  option, unknown to the agent, and True is not a TOML boolean

* Tue Oct 06 2026 Athanor Forge <forge@athanor.os> - 1.0-4
- Reword the %description and the drop-in comment: list the options set, no sealing

* Sun Sep 07 2026 Athanor Forge <forge@athanor.os> - 1.0-3
- Stop owning /etc/keylime/agent.conf.d: keylime-agent-rust-common owns it and
  the two sets of attributes conflicted at install time

* Sun Sep 06 2026 Athanor Forge <forge@athanor.os> - 1.0-2
- Install the drop-in from Source0 instead of an empty placeholder file

* Mon Aug 03 2026 Athanor Core <core@athanor.os> - 1.0-1
- Initial release for Phase 3
