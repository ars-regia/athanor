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
# ruleid: hidden-failure
probe 2>>/dev/null
# ruleid: hidden-failure
probe >&/dev/null
# ruleid: hidden-failure
probe &>/dev/null
# ruleid: hidden-failure
probe 2>"/dev/null"
# ruleid: hidden-failure
probe || exit 0
# ruleid: hidden-failure
probe || { true; }
# ruleid: hidden-failure
probe || echo "failed"; true
# ruleid: hidden-failure
usbguard set-parameter InsertedDevicePolicy block 2>/dev/null || true
case "$1" in
    unlock)
        # ok: hidden-failure
        usbguard set-parameter InsertedDevicePolicy apply-policy 2>/dev/null || true
        ;;
esac
# ok: hidden-failure
probe || exit 1
# ok: hidden-failure
while kill -0 "$pid" 2>"/dev/null"; do sleep 1; done
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
