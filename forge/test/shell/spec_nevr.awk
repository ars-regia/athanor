# spec_nevr.awk SPEC - print Version-Release of an RPM spec, or nothing.
# Release is usually a macro (%global fedora_release ...) and rpmbuild is not on the host, so the
# %global lines are expanded here, in Version and in Release: nested %{name} references up to 16
# times. Only the first Version: and the first Release: count (the main package's), and a
# %global may come before or after them. Conditionals, %{?...}, shell %(), %define and any
# undefined macro are not handled, and all of them fail closed: a value that keeps a % prints
# nothing, never a guess. A trailing comment is not stripped.
function expand(s,    n, k) {
    for (n = 0; n < 16 && match(s, /%\{[A-Za-z0-9_]+\}/); n++) {
        k = substr(s, RSTART + 2, RLENGTH - 3)
        if (!(k in m)) return "%"
        s = substr(s, 1, RSTART - 1) m[k] substr(s, RSTART + RLENGTH)
    }
    return s
}
$1 == "%global" { line = $0; sub(/^[ \t]*%global[ \t]+[^ \t]+[ \t]+/, "", line); m[$2] = line }
$1 == "Version:" && v == "" { v = $2 }
$1 == "Release:" && r == "" { r = $2 }
END {
    v = expand(v); r = expand(r)
    if (v != "" && r != "" && v !~ /%/ && r !~ /%/) print v "-" r
}
