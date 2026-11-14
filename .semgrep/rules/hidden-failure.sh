#!/usr/bin/env bash
# ruleid: hidden-failure
rm "$f" || true
# ruleid: hidden-failure
rm "$f" || :
# ruleid: hidden-failure
x=$(probe 2> /dev/null)
# ruleid: hidden-failure
probe 2> /dev/null
# ruleid: hidden-failure
probe &> /dev/null
# ruleid: hidden-failure
probe > /dev/null 2>&1
# ruleid: hidden-failure
probe 2>&-
# ok: hidden-failure
probe > /dev/null
# ok: hidden-failure
rm "$f" || {
    echo "cannot remove $f" >&2
    exit 1
}
# ok: hidden-failure
while kill -0 "$pid" 2> /dev/null; do sleep 1; done
# ok: hidden-failure
probe # a comment that names || true
# ok: hidden-failure
probe || true-ish
