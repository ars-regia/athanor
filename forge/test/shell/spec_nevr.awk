# spec_nevr.awk SPEC - print Version-Release of an RPM spec, or nothing.
# Release is usually a macro (%global fedora_release ...) and rpmbuild is not on the host, so the
# %global lines are expanded here: nested %{name} references up to 16 times. Conditionals,
# %{?...}, shell %() and an undefined macro are not handled, and all of them fail closed: the
# output is empty, never a guess.
$1 == "%global" { m[$2] = substr($0, index($0, $3)) }
$1 == "Version:" { v = $2 }
$1 == "Release:" { r = $2 }
END {
    for (n = 0; n < 16 && match(r, /%\{[A-Za-z0-9_]+\}/); n++) {
        k = substr(r, RSTART + 2, RLENGTH - 3)
        if (!(k in m)) { r = "%"; break }
        r = substr(r, 1, RSTART - 1) m[k] substr(r, RSTART + RLENGTH)
    }
    if (v != "" && r != "" && r !~ /%/) print v "-" r
}
