#!/usr/bin/python3
"""bar_e2e.py - athanor-bar end to end in the rig, as scene.sh's RIG_HOLD, with the bar
started by bar_session.py --hang CanReboot --window --pinnable and no favourites file:

- READY=1 reaches NOTIFY_SOCKET (Type=notify);
- the first start imports the favourites and saves the file (BR7);
- the preset follows the user's layout document live, a broken document falls back to the
  vendor layout without stopping the bar, and a key the policy marks mandatory holds
  (acceptance item 16);
- the test window shows as a running application under the bar preset, a title change
  updates its button in place, and a desktop entry installed while the bar runs claims a
  window by StartupWMClass; with a second
  window its button opens the menu, whose Pin to Bar and Unpin from Bar write the
  favourites file and whose row follows the pinned state;
- the accessibility popover offers high contrast, which writes COSMIC's is_high_contrast;
- the power menu asks first, with Cancel focused, and calls logind only on confirmation;
- a logind that never answers hides the action, logs it and leaves the bar running with
  the other actions still offered;
- the bar stays within 64 MB PSS under both the float and the bar preset (acceptance item 17).
"""

import os
import signal
import sys
import time
from pathlib import Path

from gi.repository import GLib

sys.path.insert(0, str(Path(__file__).resolve().parent))
from atspi_check import find_application, walk  # noqa: E402

READY_FILE = Path("/tmp/athanor-bar.ready")
PID_FILE = Path("/tmp/athanor-bar.pid")
POLICY = Path("/etc/athanor/layout/90-test.toml")
PSS_LIMIT_KB = 64 * 1024
BAR = 'schema = 1\n\n[output."*"]\npreset = "bar"\npanel = "bottom"\n'
BROKEN = "schema = 1\n[output"
MANDATORY_FLOAT = (
    'schema = 1\nmandatory = ["preset"]\n\n[output."*"]\npreset = "float"\n'
)
# The name the bar gives the test window's button (BR3, Task 8): the app name comes from
# the desktop entry bar_session.py --pinnable installs, and with one titled window the
# label is "{app}: {title}"; with two, "{app} ({count} windows)".
WINDOW = "/repo/forge/test/shell/cc_window.py"
RUNNING_WINDOW_BUTTON = "CC Window: cc-window-1"
RUNNING_WINDOWS_BUTTON = "CC Window (2 windows)"
PINNED_ID = "org.athanor.CcWindow1.desktop"

failures = []


def check(name, ok, detail=""):
    print(
        f"{'PASS' if ok else 'FAIL'} {name}{': ' + detail if detail and not ok else ''}"
    )
    if not ok:
        failures.append(name)
    return ok


def wait_for(check_now, seconds):
    deadline = time.monotonic() + seconds
    while time.monotonic() < deadline:
        if check_now():
            return True
        time.sleep(0.2)
    return check_now()


def alive(pid):
    try:
        os.kill(pid, 0)
    except ProcessLookupError:
        return False
    return True


def showing(accessible, Atspi):
    return accessible.get_state_set().contains(Atspi.StateType.SHOWING)


def buttons(app, Atspi, name):
    """Every showing push button called `name`, found afresh: a rebuild replaces them."""
    found = []

    def visit(accessible):
        try:
            if (
                accessible.get_role_name() == "button"
                and accessible.get_name() == name
                and showing(accessible, Atspi)
            ):
                found.append(accessible)
            children = [
                accessible.get_child_at_index(index)
                for index in range(accessible.get_child_count())
            ]
        except GLib.Error:
            # A window event rebuilt the row mid-walk and this node is gone: it is not
            # there to be found. The callers poll, so the next walk sees the new row.
            return
        for child in children:
            if child:
                visit(child)

    visit(app)
    return found


def name_or_none(accessible):
    """The accessible's name, or None once the bar destroyed it."""
    try:
        return accessible.get_name()
    except GLib.Error:
        return None


def retitle(number, title):
    """Retitles cc_window.py `number`'s windows through its SIGUSR1 hook."""
    Path(f"/tmp/cc-window-{number}.title").write_text(title, encoding="utf-8")
    signalled = 0
    for proc in Path("/proc").iterdir():
        if not proc.name.isdigit():
            continue
        try:
            argv = (proc / "cmdline").read_bytes().split(b"\0")
        except OSError:
            continue
        for index, arg in enumerate(argv[:-1]):
            if arg.endswith(b"cc_window.py") and argv[index + 1] == number.encode():
                os.kill(int(proc.name), signal.SIGUSR1)
                signalled += 1
    if signalled == 0:
        raise RuntimeError(f"no cc_window {number} to retitle")


def labelled(app, Atspi, role, label):
    """Every showing accessible of `role` named `label`, directly or through a LabelledBy
    relation to a label accessible: a GTK Switch takes its accessible name from the row's
    label this way, not from its own Property::Label."""
    found = []

    def name_of(accessible):
        name = accessible.get_name()
        if name:
            return name
        for relation in accessible.get_relation_set():
            if relation.get_relation_type() == Atspi.RelationType.LABELLED_BY:
                for index in range(relation.get_n_targets()):
                    target = relation.get_target(index)
                    if target is not None:
                        target_name = target.get_name()
                        if target_name:
                            return target_name
        return name

    def visit(accessible):
        try:
            if (
                accessible.get_role_name() == role
                and showing(accessible, Atspi)
                and name_of(accessible) == label
            ):
                found.append(accessible)
            children = [
                accessible.get_child_at_index(index)
                for index in range(accessible.get_child_count())
            ]
        except GLib.Error:
            # As in `buttons`: a node destroyed mid-walk is not there to be found.
            return
        for child in children:
            if child:
                visit(child)

    visit(app)
    return found


def press(app, Atspi, name):
    found = buttons(app, Atspi, name)
    if not found:
        print(f"no showing button named {name!r}; tree:", file=sys.stderr)
        for role, label, _, depth in walk(app, Atspi):
            print(f"{'  ' * depth}{role}: {label!r}", file=sys.stderr)
        return False
    found[0].do_action(0)
    return True


def confirm_button(app, Atspi, name):
    """The confirmation's button: the sibling of Cancel called `name`. The row of the
    same name sits on the hidden page of the stack."""
    for cancel in buttons(app, Atspi, "Cancel"):
        parent = cancel.get_parent()
        for index in range(parent.get_child_count()):
            child = parent.get_child_at_index(index)
            if child and child.get_role_name() == "button" and child.get_name() == name:
                return cancel, child
    return None, None


def pss_kb(pid):
    for line in (
        Path(f"/proc/{pid}/smaps_rollup").read_text(encoding="utf-8").splitlines()
    ):
        if line.startswith("Pss:"):
            return int(line.split()[1])
    return None


def high_contrast_files():
    config = Path(os.environ["XDG_CONFIG_HOME"])
    return [
        config / "cosmic" / "com.system76.CosmicTheme.Dark" / "v1" / "is_high_contrast",
        config
        / "cosmic"
        / "com.system76.CosmicTheme.Light"
        / "v1"
        / "is_high_contrast",
    ]


def high_contrast_is(value):
    def check_now():
        text = str(value).lower()
        return all(
            path.exists() and path.read_text(encoding="utf-8").strip() == text
            for path in high_contrast_files()
        )

    return check_now


def favorites_file():
    return Path(os.environ["XDG_CONFIG_HOME"]) / "athanor" / "favorites.toml"


def favorites_text():
    try:
        return favorites_file().read_text(encoding="utf-8")
    except FileNotFoundError:
        return ""


def menu_row_after(app, Atspi, opener, row):
    """Opens the menu of the button named `opener` and returns its row named `row`. A
    window event can rebuild the row of buttons between the lookup and the press, which
    then lands on a button already gone: press again, up to three times."""
    for _ in range(3):
        if press(app, Atspi, opener) and wait_for(lambda: buttons(app, Atspi, row), 2):
            return buttons(app, Atspi, row)[0]
    print(f"no showing button named {row!r} after {opener!r}; tree:", file=sys.stderr)
    for role, label, _, depth in walk(app, Atspi):
        print(f"{'  ' * depth}{role}: {label!r}", file=sys.stderr)
    return None


def main():
    import subprocess

    import gi

    gi.require_version("Atspi", "2.0")
    from gi.repository import Atspi

    user = Path(os.environ["XDG_CONFIG_HOME"]) / "athanor" / "layout.toml"
    log = Path("/out") / f"{os.environ['RIG_TAG']}-logind.log"
    client_log = Path("/out") / f"{os.environ['RIG_TAG']}-client.log"

    if not check("READY=1 on NOTIFY_SOCKET", wait_for(READY_FILE.exists, 10)):
        return 1
    pid = int(PID_FILE.read_text(encoding="utf-8"))
    app = find_application(Atspi, "athanor-bar")
    if not check("the bar is on the accessibility bus", app is not None):
        return 1
    check(
        "the first start saved the imported favourites",
        wait_for(lambda: favorites_text().startswith("schema = 1"), 5),
        repr(favorites_text()),
    )

    check(
        "float: Workspaces shows",
        wait_for(lambda: buttons(app, Atspi, "Workspaces"), 5),
    )
    pss_float = pss_kb(pid)
    print(f"athanor-bar PSS (float): {pss_float} kB")

    user.write_text(BAR, encoding="utf-8")
    check(
        "bar: Launcher shows within 2 s of the edit",
        wait_for(lambda: buttons(app, Atspi, "Launcher"), 2),
    )
    check(
        "bar: the test window is a running application",
        wait_for(lambda: buttons(app, Atspi, RUNNING_WINDOW_BUTTON), 5),
    )
    before = buttons(app, Atspi, RUNNING_WINDOW_BUTTON)
    retitle("1", "cc-window-1 renamed")
    check(
        "a title change updates the button in place, not a new one (BR3)",
        bool(before)
        and wait_for(
            lambda: name_or_none(before[0]) == "CC Window: cc-window-1 renamed", 3
        ),
        repr(name_or_none(before[0])) if before else "no button",
    )
    retitle("1", "cc-window-1")
    check(
        "the title restores to cc-window-1",
        wait_for(lambda: buttons(app, Atspi, RUNNING_WINDOW_BUTTON), 3),
    )

    third = subprocess.Popen(["python3", WINDOW, "3"])
    check(
        "a window with no desktop entry shows under its app id",
        wait_for(lambda: buttons(app, Atspi, "org.athanor.CcWindow3: cc-window-3"), 5),
    )
    entry = Path(os.environ["XDG_DATA_HOME"]) / "applications" / "cc-three.desktop"
    entry.write_text(
        "[Desktop Entry]\nType=Application\nName=CC Three\nExec=true\n"
        "StartupWMClass=org.athanor.CcWindow3\n",
        encoding="utf-8",
    )
    check(
        "an entry installed while the bar runs claims the window by StartupWMClass",
        wait_for(lambda: buttons(app, Atspi, "CC Three: cc-window-3"), 10),
    )
    third.terminate()
    third.wait(5)
    entry.unlink()
    pss_bar = pss_kb(pid)
    print(f"athanor-bar PSS (bar, window shown): {pss_bar} kB")

    # A second start of the same app asks the first for another window: with two, a press
    # opens the button's menu (BR3).
    subprocess.Popen(["python3", WINDOW, "1"])
    check(
        "a second window groups under the same button",
        wait_for(lambda: buttons(app, Atspi, RUNNING_WINDOWS_BUTTON), 5),
    )
    check(
        "the test app is not pinned before the menu pins it",
        PINNED_ID not in favorites_text(),
        repr(favorites_text()),
    )
    pin = menu_row_after(app, Atspi, RUNNING_WINDOWS_BUTTON, "Pin to Bar")
    if check("the menu offers Pin to Bar", pin is not None):
        pin.do_action(0)
    check(
        "Pin to Bar writes the id to the favourites file",
        wait_for(lambda: PINNED_ID in favorites_text(), 3),
        repr(favorites_text()),
    )
    unpin = menu_row_after(app, Atspi, RUNNING_WINDOWS_BUTTON, "Unpin from Bar")
    check(
        "the pinned app's menu offers Unpin from Bar, not Pin to Bar",
        unpin is not None and not buttons(app, Atspi, "Pin to Bar"),
    )
    if unpin is not None:
        unpin.do_action(0)
    check(
        "Unpin from Bar removes the id from the favourites file",
        wait_for(
            lambda: (
                favorites_text().startswith("schema = 1")
                and PINNED_ID not in favorites_text()
            ),
            3,
        ),
        repr(favorites_text()),
    )
    check("the bar survives pinning and unpinning", alive(pid))

    user.write_text(BROKEN, encoding="utf-8")
    check(
        "a broken document falls back to float",
        wait_for(
            lambda: (
                buttons(app, Atspi, "Workspaces")
                and not buttons(app, Atspi, "Launcher")
            ),
            2,
        ),
    )
    check(
        "the broken document is left as it was",
        user.read_text(encoding="utf-8") == BROKEN,
    )
    check("the bar survives a broken document", alive(pid))

    user.write_text(BAR, encoding="utf-8")
    wait_for(lambda: buttons(app, Atspi, "Launcher"), 2)
    POLICY.parent.mkdir(parents=True, exist_ok=True)
    POLICY.write_text(MANDATORY_FLOAT, encoding="utf-8")
    # A directory created under a running watch is found by GLib's poll of missing paths,
    # every few seconds: hence 10 s here, not 2.
    check(
        "a mandatory preset holds over the user's document (item 16)",
        wait_for(
            lambda: (
                buttons(app, Atspi, "Workspaces")
                and not buttons(app, Atspi, "Launcher")
            ),
            10,
        ),
    )

    check("the accessibility popover opens", press(app, Atspi, "Accessibility"))
    # GtkSwitch reports as "check box" on the rig's at-spi2-core, the same naming
    # variance atspi_check.py already notes for the push button role.
    check(
        "High contrast shows",
        wait_for(lambda: labelled(app, Atspi, "check box", "High contrast"), 5),
    )
    switches = labelled(app, Atspi, "check box", "High contrast")
    if switches:
        switches[0].do_action(0)
    check(
        "high contrast reaches both of COSMIC's theme files within 2 s",
        wait_for(high_contrast_is(True), 2),
    )
    check("the bar stays alive with high contrast on", alive(pid))
    switches = labelled(app, Atspi, "check box", "High contrast")
    if switches:
        switches[0].do_action(0)
    # Switched back off so the checks after this one run in the normal, non-high-contrast
    # variant, same as every other capture in the rig.
    check("high contrast is switched back off", wait_for(high_contrast_is(False), 2))

    check("the power menu opens", press(app, Atspi, "Power"))
    check(
        "Shut Down is offered on challenge",
        wait_for(lambda: buttons(app, Atspi, "Shut Down"), 5),
    )
    check("Suspend is offered", press(app, Atspi, "Suspend"))
    cancel, confirm = None, None

    def asked():
        nonlocal cancel, confirm
        cancel, confirm = confirm_button(app, Atspi, "Suspend")
        return confirm is not None

    check("Suspend asks first", wait_for(asked, 3))
    check(
        "Cancel has the focus",
        cancel is not None and cancel.get_state_set().contains(Atspi.StateType.FOCUSED),
    )
    time.sleep(1)
    # bar_session.py creates the log before the bar starts: missing is a failure.
    check(
        "nothing reached logind before the confirmation",
        log.exists() and "Suspend" not in log.read_text(),
    )
    if confirm is not None:
        confirm.do_action(0)
    check(
        "the confirmation suspends through logind",
        wait_for(
            lambda: log.exists() and "Suspend True" in log.read_text().splitlines(), 5
        ),
    )

    # Every opening asks logind again, with a 5 s timeout. Restart starts hidden, so it
    # being hidden proves nothing on its own: wait for this opening's timeout line, which
    # lands 5 s after the opening (an earlier opening's lands sooner), then look.
    def reboot_timeouts():
        if not client_log.exists():
            return 0
        text = client_log.read_text(encoding="utf-8", errors="replace")
        return sum(
            1
            for line in text.splitlines()
            if "did not say" in line and "CanReboot" in line
        )

    opened = time.monotonic()
    press(app, Atspi, "Power")
    check(
        "the power menu opens again",
        wait_for(lambda: buttons(app, Atspi, "Suspend"), 3),
    )
    seen = reboot_timeouts()

    def this_opening_timed_out():
        nonlocal seen
        now = reboot_timeouts()
        timed_out = now > seen and time.monotonic() - opened >= 4.5
        seen = now
        return timed_out

    check(
        "the bar logged this opening's CanReboot timeout",
        wait_for(this_opening_timed_out, 8),
    )
    check(
        "after the timeout, Restart is hidden",
        not buttons(app, Atspi, "Restart"),
    )
    check(
        "after the timeout, Suspend, which logind answered, still shows",
        bool(buttons(app, Atspi, "Suspend")),
    )
    check("and the bar keeps running", alive(pid))

    print(f"athanor-bar PSS: float={pss_float} kB, bar with window={pss_bar} kB")
    check(
        "PSS at rest within 64 MB under both presets (item 17)",
        pss_float is not None
        and pss_bar is not None
        and max(pss_float, pss_bar) <= PSS_LIMIT_KB,
        f"float={pss_float} kB bar={pss_bar} kB",
    )
    if failures:
        print(f"bar-e2e: {len(failures)} failed", file=sys.stderr)
        return 1
    print("bar-e2e: every check passed")
    return 0


if __name__ == "__main__":
    sys.exit(main())
