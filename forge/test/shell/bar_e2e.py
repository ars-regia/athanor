#!/usr/bin/python3
"""bar_e2e.py - athanor-bar end to end in the rig, as scene.sh's RIG_HOLD, with the bar
started by bar_session.py --hang CanReboot --window:

- READY=1 reaches NOTIFY_SOCKET (Type=notify);
- the preset follows the user's layout document live, a broken document falls back to the
  vendor layout without stopping the bar, and a key the policy marks mandatory holds
  (acceptance item 16);
- the test window shows as a running application under the bar preset;
- the accessibility popover offers high contrast, which writes COSMIC's is_high_contrast;
- the power menu asks first, with Cancel focused, and calls logind only on confirmation;
- a logind that never answers hides the action, logs it and leaves the bar running with
  the other actions still offered;
- the bar stays within 64 MB PSS under both the float and the bar preset (acceptance item 17).
"""

import os
import sys
import time
from pathlib import Path

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
# The name the bar gives the test window's button (BR3, Task 8): no installed desktop
# entry claims the id cc_window.py uses, so the app name is the raw app id, and with one
# titled window the label is "{app}: {title}".
RUNNING_WINDOW_BUTTON = "org.athanor.CcWindow1: cc-window-1"

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
        if (
            accessible.get_role_name() == "button"
            and accessible.get_name() == name
            and showing(accessible, Atspi)
        ):
            found.append(accessible)
        for index in range(accessible.get_child_count()):
            child = accessible.get_child_at_index(index)
            if child:
                visit(child)

    visit(app)
    return found


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
        if (
            accessible.get_role_name() == role
            and showing(accessible, Atspi)
            and name_of(accessible) == label
        ):
            found.append(accessible)
        for index in range(accessible.get_child_count()):
            child = accessible.get_child_at_index(index)
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


def main():
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
    pss_bar = pss_kb(pid)
    print(f"athanor-bar PSS (bar, window shown): {pss_bar} kB")

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
    check(
        "nothing reached logind before the confirmation",
        not log.exists() or "Suspend" not in log.read_text(),
    )
    if confirm is not None:
        confirm.do_action(0)
    check(
        "the confirmation suspends through logind",
        wait_for(
            lambda: log.exists() and "Suspend True" in log.read_text().splitlines(), 5
        ),
    )

    press(app, Atspi, "Power")
    time.sleep(6)
    check(
        "a CanReboot that never answers hides Restart",
        not buttons(app, Atspi, "Restart"),
    )
    check(
        "the bar logged the CanReboot timeout",
        client_log.exists()
        and "CanReboot" in client_log.read_text(encoding="utf-8", errors="replace"),
    )
    check(
        "Suspend is still offered after the CanReboot timeout",
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
