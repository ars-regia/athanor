#!/usr/bin/env bash
# update.sh - regenerates the template from the sources and merges it into every
# catalog. Needs GNU gettext 0.24 or later (xgettext --language=Rust); the rig's build
# image has 0.25. Run after adding or changing a tr()/tr_with() call.
set -euo pipefail
here=$(dirname "${BASH_SOURCE[0]}")
crate=$here/..
xgettext --language=Rust --keyword=tr --keyword=tr_with --from-code=UTF-8 --add-comments=TRANSLATORS \
	--package-name=athanor-layout-chooser --msgid-bugs-address=forge@athanor.os --no-wrap --sort-by-file \
	--directory="$crate" --files-from="$here/POTFILES.in" --output="$here/athanor-layout-chooser.pot"
# The creation date would make every run a diff.
sed -i '/^"POT-Creation-Date:/d' "$here/athanor-layout-chooser.pot"
for catalog in "$here"/*.po; do
	msgmerge --update --backup=none --no-wrap "$catalog" "$here/athanor-layout-chooser.pot"
done
