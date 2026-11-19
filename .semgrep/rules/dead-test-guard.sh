#!/usr/bin/env bash
set -euo pipefail
scanned=0 found=0
# ruleid: dead-test-guard
[ "$scanned" -gt 0 ] && [ "$found" = 0 ]
mkdir "$out"
# ruleid: dead-test-guard
[[ -n "$a" ]] && [[ "$b" = 1 ]]
# a comment between the guard and the next command
echo done
# ruleid: dead-test-guard
[ -n "$a" ] && [ -n "$b" ] # a trailing comment
echo done
# ruleid: dead-test-guard
[ -n "$a" ] && [ -n "$b" ]; echo done
if [ -n "$c" ]; then
  # ruleid: dead-test-guard
  [ -n "$a" ] && [ -n "$b" ]
fi
for d in a b; do
  # ruleid: dead-test-guard
  [ -n "$d" ] && [ -n "$b" ]
done
case "$c" in
  x)
    # ruleid: dead-test-guard
    [ -n "$a" ] && [ -n "$b" ]
    ;;
esac
while
  # ok: dead-test-guard
  [ -n "$a" ] && [ -n "$b" ]
do
  break
done
x=$(
  # ok: dead-test-guard
  [[ -n "$a" ]] && [[ -n "$b" ]]
)
# ok: dead-test-guard
[ "$scanned" -gt 0 ] || { echo "no library scanned" >&2; exit 1; }
# ok: dead-test-guard
[ "$found" = 0 ]
check() {
  # ok: dead-test-guard
  [ "$n" -gt 0 ] && [ "$bad" = 0 ]
}
# ok: dead-test-guard
[ -d "$dir" ] && [ -r "$dir" ] && rm -- "$dir/stale"
# ok: dead-test-guard
[ "$n" -gt 0 ] && [ "$bad" = 0 ]
