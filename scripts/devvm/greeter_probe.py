"""greeter_probe.py: run as root in the guest by greeter-acceptance.sh, beside keyboard_type.py
(`sudo python3 greeter_probe.py COMMAND [ARG...]`), to drive and read athanor-greeter-ui.

Keys go through a throwaway uinput keyboard (keyboard_type.py's key tables), opened as root
before the probe drops to the greetd user; the greeter is read over AT-SPI on greetd's session
bus, as that user, the way a screen reader at the greeter would read it. Prints JSON, or one
number per line for `attempts`; exits 1 with a reason on stderr when what it waited for did
not appear.

  tree                      every node of the greeter: role, name, showing, selected, focused
  cards                     the open card, then Escape to the list: card names in order, the selected one
  open-card FULL-NAME       to the list with Escape, to the card by its first letter, Enter
  open-other                the same for "Other user"; leaves its name field showing
  alerts SECONDS            waits for a "Sign-in failed" text; prints every showing alert
  attempts NAME PW COUNT    COUNT sign-ins through "Other user"; prints, one per line, the
                            seconds from the Enter that submits the password to the failure
                            text, a tab, and the name of the password question answered
  compare A B               host side: the medians of two files of seconds against A's spread
"""

import fcntl
import json
import os
import pwd
import statistics
import struct
import sys
import time

APP = "os.athanor.Greeter"
OTHER = "Other user"
FAILED = "Sign-in failed"
# How long a screen takes to change after a key.
WAIT_S = 10
POLL_S = 0.05


class Keyboard:
    """A uinput keyboard, created before the privileges drop."""

    def __init__(self):
        import keyboard_type as kt

        self.kt = kt
        self.fd = os.open("/dev/uinput", os.O_WRONLY | os.O_NONBLOCK)
        fcntl.ioctl(self.fd, kt.UI_SET_EVBIT, kt.EV_SYN)
        fcntl.ioctl(self.fd, kt.UI_SET_EVBIT, kt.EV_KEY)
        for code in {*kt.NAMED.values(), *kt.PLAIN.values(), *kt.SHIFTED.values()}:
            fcntl.ioctl(self.fd, kt.UI_SET_KEYBIT, code)
        zeros = [0] * kt.ABS_CNT
        os.write(
            self.fd,
            struct.pack(
                f"80sHHHHi{4 * kt.ABS_CNT}i",
                b"athanor-acceptance-keyboard",
                kt.BUS_VIRTUAL,
                0,
                0,
                1,
                0,
                *zeros,
                *zeros,
                *zeros,
                *zeros,
            ),
        )
        fcntl.ioctl(self.fd, kt.UI_DEV_CREATE)
        time.sleep(kt.SETTLE_S)

    def type(self, *tokens):
        for codes, repeats, interval, pause in self.kt.strokes(tokens):
            self.kt.press(self.fd, codes, repeats, interval)
            time.sleep(pause)

    def close(self):
        fcntl.ioctl(self.fd, self.kt.UI_DEV_DESTROY)
        os.close(self.fd)


def drop_to_greetd():
    entry = pwd.getpwnam("greetd")
    runtime = f"/run/user/{entry.pw_uid}"
    os.environ["XDG_RUNTIME_DIR"] = runtime
    os.environ["DBUS_SESSION_BUS_ADDRESS"] = f"unix:path={runtime}/bus"
    os.setgroups([])
    os.setgid(entry.pw_gid)
    os.setuid(entry.pw_uid)


class Greeter:
    def __init__(self):
        import gi

        gi.require_version("Atspi", "2.0")
        from gi.repository import Atspi, GLib

        self.Atspi, self.GLib = Atspi, GLib

    def application(self):
        desktop = self.Atspi.get_desktop(0)
        for index in range(desktop.get_child_count()):
            app = desktop.get_child_at_index(index)
            if app is not None and app.get_name() in (APP, "athanor-greeter-ui"):
                return app
        return None

    def text(self, node, name):
        if name:
            return name
        try:
            return self.Atspi.Text.get_text(node, 0, -1)
        except self.GLib.Error:
            return ""  # a node without the Text interface has no text to read

    def tree(self):
        app = self.application()
        if app is None:
            return None
        out = []

        def walk(node):
            states = node.get_state_set()
            name = node.get_name() or ""
            role = node.get_role_name()
            out.append(
                {
                    "role": role,
                    "name": name,
                    "text": self.text(node, name) if role in ("label", "alert") else "",
                    "showing": states.contains(self.Atspi.StateType.SHOWING),
                    "selected": states.contains(self.Atspi.StateType.SELECTED),
                    "focused": states.contains(self.Atspi.StateType.FOCUSED),
                }
            )
            for index in range(node.get_child_count()):
                child = node.get_child_at_index(index)
                if child is not None:
                    walk(child)

        walk(app)
        return out

    def wait(self, what, check, seconds=WAIT_S, required=True):
        """The value `check` returns for the tree, once it is not None; None after SECONDS
        when the wait is not required."""
        deadline = time.monotonic() + seconds
        while True:
            tree = self.tree()
            if tree is not None:
                found = check(tree)
                if found is not None:
                    return found
            if time.monotonic() > deadline:
                if required:
                    fail(f"{what} did not appear in {seconds} s")
                return None
            time.sleep(POLL_S)


def fail(reason):
    print(f"greeter_probe.py: {reason}", file=sys.stderr)
    sys.exit(1)


def rows(tree):
    return [n for n in tree if n["role"] == "list item" and n["showing"]]


def list_shown(tree):
    return any(n["name"] == "Accounts" and n["showing"] for n in tree)


def name_field(tree):
    return any(
        n["name"] == "User name" and n["role"] in ("entry", "text") and n["showing"]
        for n in tree
    )


def open_card_name(tree):
    for n in tree:
        if (
            n["role"] == "password text"
            and n["showing"]
            and n["name"].startswith("Password for ")
        ):
            return n["name"]
    return None


def alerts(tree):
    return [
        n["text"] for n in tree if n["role"] == "alert" and n["showing"] and n["text"]
    ]


def failed(tree):
    found = alerts(tree)
    return found if any(FAILED in line for line in found) else None


def to_list(greeter, keys):
    tree = greeter.wait("the greeter", lambda t: t)
    if not list_shown(tree):
        keys.type("@escape")
        greeter.wait("the list after Escape", lambda t: list_shown(t) or None)


def pick(greeter, keys, row_name):
    """Selects the row by its first letter, as the greeter's typed-letter rule moves."""
    letter = row_name[0].lower()
    for _ in range(len(rows(greeter.wait("the list", lambda t: t))) + 1):
        keys.type(letter)
        time.sleep(0.3)
        if any(n["name"] == row_name and n["selected"] for n in rows(greeter.tree())):
            return
    fail(f"the letter {letter!r} never selected {row_name!r}")


def open_other(greeter, keys):
    if name_field(greeter.wait("the greeter", lambda t: t)):
        return
    to_list(greeter, keys)
    pick(greeter, keys, OTHER)
    keys.type("@enter")
    greeter.wait("Other user's name field", lambda t: name_field(t) or None)


def cmd_open_card(greeter, keys, full_name):
    to_list(greeter, keys)
    # The row is named "FULL, user"; the typed letter is the full name's first.
    row = next(
        (
            n["name"]
            for n in rows(greeter.tree())
            if n["name"].startswith(f"{full_name}, ")
        ),
        None,
    )
    if row is None:
        fail(f"no card for {full_name!r}")
    pick(greeter, keys, row)
    keys.type("@enter")
    greeter.wait(
        f"the password field of {full_name}",
        lambda t: True if open_card_name(t) == f"Password for {full_name}" else None,
    )


def cmd_attempts(greeter, keys, name, password, count):
    out = []
    for _ in range(count):
        open_other(greeter, keys)
        keys.type(name, "@enter")
        question = greeter.wait(
            "PAM's password question",
            lambda t: next(
                (n["name"] for n in t if n["role"] == "password text" and n["showing"]),
                None,
            ),
        )
        keys.type(password)
        start = time.monotonic()
        keys.type("@enter")
        greeter.wait("the failure text", failed, 30)
        out.append((time.monotonic() - start, question))
        # The failure returns to the name field, emptied.
        greeter.wait(
            "the name field after the failure", lambda t: name_field(t) or None
        )
    return out


def compare(first, second):
    """The difference of the medians against the first sample's interquartile range."""
    a = [float(line.split("\t")[0]) for line in open(first).read().splitlines()]
    b = [float(line.split("\t")[0]) for line in open(second).read().splitlines()]
    quartiles = statistics.quantiles(a, n=4)
    spread = quartiles[2] - quartiles[0]
    gap = abs(statistics.median(a) - statistics.median(b))
    print(
        json.dumps(
            {
                "median_a": statistics.median(a),
                "median_b": statistics.median(b),
                "gap": gap,
                "iqr_a": spread,
            }
        )
    )
    return 0 if gap <= spread else 1


def main(argv):
    if not argv:
        print(__doc__, file=sys.stderr)
        return 2
    if argv[0] == "compare":
        return compare(argv[1], argv[2])
    keys = Keyboard()
    try:
        drop_to_greetd()
        greeter = Greeter()
        match argv[0]:
            case "tree":
                tree = greeter.tree()
                if tree is None:
                    fail(f"no application {APP} on greetd's accessibility bus")
                print(json.dumps(tree))
            case "cards":
                tree = greeter.wait("the greeter", lambda t: t)
                opened = open_card_name(tree)
                if not list_shown(tree):
                    # The list is behind the open card: Escape goes back to it. With the
                    # list hidden there is none, and the rows stay empty.
                    keys.type("@escape")
                    tree = (
                        greeter.wait(
                            "the list", lambda t: t if list_shown(t) else None, 3, False
                        )
                        or tree
                    )
                print(
                    json.dumps(
                        {
                            "list": [n["name"] for n in rows(tree)],
                            "selected": next(
                                (n["name"] for n in rows(tree) if n["selected"]), None
                            ),
                            "open": opened,
                            "list_shown": list_shown(tree),
                        }
                    )
                )
            case "open-card":
                cmd_open_card(greeter, keys, argv[1])
            case "open-other":
                open_other(greeter, keys)
            case "alerts":
                print(
                    json.dumps(greeter.wait("the failure text", failed, int(argv[1])))
                )
            case "attempts":
                for seconds, question in cmd_attempts(
                    greeter, keys, argv[1], argv[2], int(argv[3])
                ):
                    print(f"{seconds:.3f}\t{question}")
            case _:
                fail(f"unknown command {argv[0]!r}")
    finally:
        keys.close()
    return 0


if __name__ == "__main__":
    sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
    sys.exit(main(sys.argv[1:]))
