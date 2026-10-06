# Security Policy

## Supported versions

Athanor has no release yet. Security fixes land on the development branch `iso-v0`; no
other branch receives them.

## Reporting a vulnerability

Do not open a public issue. Report the vulnerability privately through GitHub's private
vulnerability reporting ("Report a vulnerability" under the repository's Security tab),
with a description and the steps to reproduce it.

In scope are the parts that ship in the system image: the kernel and its signing keys, the
image build and signing pipeline, the update path (`athanor-update`), the desktop portal
backend and the shell. Crates listed in `experimental/EXEMPT` are not part of the image.

Athanor is maintained by one person, and reports are answered on a best-effort basis.
