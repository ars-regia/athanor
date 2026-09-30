#!/usr/bin/python3
"""shield_e2e.py - package 2b.5's shield in a scene (doc_bar.md BR3 Power, BR6, item 13):
the bar against trust_state.py's files, served by bar_session.py's fake os.athanor.Update1
(State, Apply, GoBack), driven through AT-SPI. An impostor of another user, update_impostor.py,
checks that the bar trusts only root's answer. Runs as scene.sh's RIG_HOLD with
`bar_session.py --notifications --trust-state downloaded`. One line per check; exits 1 if
any fails. GTK clicks a button activated through AT-SPI only after its 250 ms press
animation, so every check after a press waits.
"""

import json
import os
import re
import subprocess
import sys
import time
from pathlib import Path

from gi.repository import GLib

sys.path.insert(0, str(Path(__file__).resolve().parent))
from atspi_check import find_application, walk  # noqa: E402
from bar_e2e import (  # noqa: E402
    buttons,
    buttons_matching,
    check,
    confirm_button,
    failures,
    labelled,
    press,
    wait_for,
)
from bar_session import STATE_ERROR_FILE, SYSTEM_BUS  # noqa: E402
from notifications_e2e import alerts, never, notify  # noqa: E402
import trust_state  # noqa: E402

LOG = Path("/out") / f"{os.environ.get('RIG_TAG', 'shield-e2e')}-logind.log"
REFUSE_FILE = Path("/tmp/athanor-update-refuse")
UNTRUSTED = "The trust state file is not owned by the system and was ignored"
UNREADABLE = "The trust state file could not be read"
NO_ANSWER = "The update service did not answer"
IMPOSTOR = Path(__file__).resolve().parent / "update_impostor.py"
SHIELD_NAMES = re.compile(r"^(System image verified|Not verified yet|Update refused)$")
# Rows only the sheet shows: the Secure Boot row of a read state, or why none was read.
SHEET_ROWS = (
    "Secure Boot on",
    "Secure Boot off: this machine runs in the declared degraded mode",
    "No trust state yet: the first check has not run",
    "The trust state file is not owned by the system and was ignored",
    "The trust state file could not be read",
    "The update service did not answer",
)


def logged(line):
    return line in LOG.read_text(encoding="utf-8").splitlines()


def applies():
    return LOG.read_text(encoding="utf-8").splitlines().count("Apply")


def shield(app, Atspi):
    found = buttons_matching(app, Atspi, SHIELD_NAMES)
    return found[0] if found else None


def shield_named(app, Atspi, name):
    return bool(buttons(app, Atspi, name))


def sheet_open(app, Atspi):
    return any(labelled(app, Atspi, "label", row) for row in SHEET_ROWS)


def toggle_sheet(app, Atspi, open_):
    button = shield(app, Atspi)
    if button is None:
        return False
    button.do_action(0)
    return wait_for(lambda: sheet_open(app, Atspi) == open_, 3)


def focused(app, Atspi, name):
    return any(
        button.get_state_set().contains(Atspi.StateType.FOCUSED)
        for button in buttons(app, Atspi, name)
    )


def said(app, Atspi, row):
    """The open sheet shows `row`: waited for, as a sheet that just opened may still show the
    rows of the state before."""
    return wait_for(lambda: bool(labelled(app, Atspi, "label", row)), 3)


def verified_first(app, Atspi, before):
    """Writes a verified state and checks the shield says so before `before` happens."""
    trust_state.write("verified")
    check(
        f"the shield is verified before {before}",
        wait_for(lambda: shield_named(app, Atspi, "System image verified"), 3),
    )


def confirm(app, Atspi, name):
    """Presses the confirmation's `name` once it shows; False when it never does."""
    if not wait_for(lambda: confirm_button(app, Atspi, name)[1] is not None, 3):
        return False
    button = confirm_button(app, Atspi, name)[1]
    return button is not None and button.do_action(0)


def shows(app, Atspi, summary):
    return summary in (alerts(app, Atspi) or [])


def labels(app, Atspi):
    """The showing labels' names, or None when a rebuild removed a node mid-walk."""
    try:
        return [
            name
            for role, name, shown, _ in walk(app, Atspi)
            if shown and role == "label"
        ]
    except GLib.Error:
        return None


def main():
    import gi

    gi.require_version("Atspi", "2.0")
    from gi.repository import Atspi

    app = find_application(Atspi, "athanor-bar")
    if not check("the bar is on the accessibility bus", app is not None):
        return 1

    check(
        "the shield is named by its header",
        wait_for(lambda: shield_named(app, Atspi, "System image verified"), 5),
    )

    # The sheet, and the popups held while it is open (item 13).
    check("the sheet opens", toggle_sheet(app, Atspi, True))
    for row in (
        "Version 43.20260915.2",
        "Signed with a key of the policy in force",
        "An update is ready; it installs when you restart",
        "Signature policy in force",
        "The policy is the one Athanor ships",
        "Secure Boot on",
    ):
        check(f"the sheet says {row!r}", bool(labelled(app, Atspi, "label", row)))
    notify("While the sheet is open")
    check(
        "a popup waits while the sheet is open",
        never(lambda: shows(app, Atspi, "While the sheet is open"), 2),
        repr(alerts(app, Atspi)),
    )
    check("the sheet closes on a second press", toggle_sheet(app, Atspi, False))
    check(
        "the waiting popup shows once it closes",
        wait_for(lambda: shows(app, Atspi, "While the sheet is open"), 3),
    )

    # Stacking: the power menu closes the sheet.
    check("the sheet opens again", toggle_sheet(app, Atspi, True))
    check(
        "opening Power closes the sheet",
        press(app, Atspi, "Power") and wait_for(lambda: not sheet_open(app, Atspi), 3),
    )

    # Restart to update in the power menu (BR3) calls Apply.
    check(
        "the power menu offers Restart to update",
        wait_for(lambda: bool(buttons(app, Atspi, "Restart to update")), 3),
    )
    press(app, Atspi, "Restart to update")
    check("it asks first, and confirming", confirm(app, Atspi, "Restart to update"))
    check("calls Apply", wait_for(lambda: logged("Apply"), 3))

    # Go back from the sheet calls GoBack.
    check("the sheet opens for Go back", toggle_sheet(app, Atspi, True))
    press(app, Atspi, "Go back to the previous version")
    check("Go back asks first, and confirming", confirm(app, Atspi, "Go back"))
    check("calls GoBack", wait_for(lambda: logged("GoBack"), 3))

    # Cancel gives the keyboard back to the action, and a redraw that changes nothing keeps it.
    go_back = "Go back to the previous version"
    press(app, Atspi, go_back)
    check(
        "Go back asks again",
        wait_for(lambda: confirm_button(app, Atspi, "Go back")[1] is not None, 3),
    )
    check("Cancel backs out", press(app, Atspi, "Cancel"))
    check(
        "and gives the focus back to Go back",
        wait_for(lambda: focused(app, Atspi, go_back), 3),
    )
    trust_state.write("downloaded")
    time.sleep(1)
    check("a state that changed nothing leaves the focus there", focused(app, Atspi, go_back))

    # A refusal is said in the sheet (Review Focus 4).
    REFUSE_FILE.write_text("Blocked\n", encoding="utf-8")
    press(app, Atspi, "Restart to update")
    confirm(app, Atspi, "Restart to update")
    # The refusal is an Alert, so a screen reader says it when it appears.
    check(
        "a blocked restart is said in the sheet",
        wait_for(
            lambda: shows(
                app, Atspi, "A program is blocking the restart; close it and try again"
            ),
            5,
        ),
    )

    # The polkit agent takes the focus, which closes the sheet before a refusal of Go back
    # arrives: the refusal opens the sheet again to say it.
    REFUSE_FILE.write_text("NotAuthorized 3\n", encoding="utf-8")
    check("the sheet closes before Go back", toggle_sheet(app, Atspi, False))
    check("and opens for it", toggle_sheet(app, Atspi, True))
    press(app, Atspi, "Go back to the previous version")
    check("Go back is confirmed", confirm(app, Atspi, "Go back"))
    check("the sheet closes while polkit would ask", toggle_sheet(app, Atspi, False))
    check(
        "the refusal opens the sheet again and is said there",
        wait_for(lambda: shows(app, Atspi, "Not authorised"), 6),
    )
    REFUSE_FILE.unlink()

    # The file changes under an open confirmation (Review Focus 1). The new state's rows are
    # on the hidden page, so the wait is fixed; a state not re-read by then calls Apply, and
    # the first check below fails.
    before = applies()
    check("the sheet offers Restart to update", press(app, Atspi, "Restart to update"))
    check(
        "which asks first",
        wait_for(
            lambda: confirm_button(app, Atspi, "Restart to update")[1] is not None, 3
        ),
    )
    trust_state.write("verified")
    time.sleep(1)
    _, button = confirm_button(app, Atspi, "Restart to update")
    check(
        "the confirmation is confirmed after the download vanished",
        button is not None and button.do_action(0),
    )
    check(
        "and calls nothing",
        never(lambda: applies() > before, 2),
    )
    check(
        "but says nothing is downloaded",
        wait_for(
            lambda: shows(
                app,
                Atspi,
                "Nothing is downloaded yet; the update downloads at the next check",
            ),
            3,
        ),
    )
    toggle_sheet(app, Atspi, False)

    # Live refresh of the seal and the header: each state names it differently from the one
    # before, and the file's removal below follows a verified state.
    for name, header in (
        ("refused", "Update refused"),
        ("attention", "Not verified yet"),
        ("verified", "System image verified"),
    ):
        trust_state.write(name)
        check(
            f"state {name} names the shield {header!r}",
            wait_for(lambda h=header: shield_named(app, Atspi, h), 3),
        )
    trust_state.write("missing")
    check(
        "the shield follows the file's removal",
        wait_for(lambda: shield_named(app, Atspi, "Not verified yet"), 3),
    )
    check("the sheet opens on a missing file", toggle_sheet(app, Atspi, True))
    check(
        "and says why",
        said(app, Atspi, "No trust state yet: the first check has not run"),
    )
    check("and closes", toggle_sheet(app, Atspi, False))

    # The state comes from State(): its errors keep their rows. Each starts from a verified
    # shield, so "Not verified yet" is the error's doing.
    for error, row in (
        ("Untrusted", UNTRUSTED),
        ("Unreadable", UNREADABLE),
        ("org.freedesktop.DBus.Error.NoReply", NO_ANSWER),
    ):
        verified_first(app, Atspi, f"State fails with {error}")
        STATE_ERROR_FILE.write_text(f"{error}\n", encoding="utf-8")
        trust_state.write("verified")
        check(
            f"State failing with {error} names the shield 'Not verified yet'",
            wait_for(lambda: shield_named(app, Atspi, "Not verified yet"), 3),
        )
        check("and the sheet opens", toggle_sheet(app, Atspi, True))
        check(f"and says {row!r}", said(app, Atspi, row))
        check("and closes", toggle_sheet(app, Atspi, False))
        STATE_ERROR_FILE.unlink()

    # No answer is asked again 5 s later, without the file changing.
    verified_first(app, Atspi, "the service stops answering")
    STATE_ERROR_FILE.write_text("org.freedesktop.DBus.Error.NoReply\n", encoding="utf-8")
    trust_state.write("verified")
    check(
        "no answer names the shield 'Not verified yet'",
        wait_for(lambda: shield_named(app, Atspi, "Not verified yet"), 3),
    )
    # The questions the rename's events started are answered by now; only a retry asks again.
    time.sleep(1)
    STATE_ERROR_FILE.unlink()
    check(
        "the shield asks again on its own and reads verified",
        wait_for(lambda: shield_named(app, Atspi, "System image verified"), 10),
    )

    # Only root's answer is trusted: a verified state from another user is not.
    verified_first(app, Atspi, "an impostor answers")
    impostor = subprocess.Popen(
        [
            "setpriv",
            "--reuid=65534",
            "--regid=65534",
            "--clear-groups",
            "python3",
            str(IMPOSTOR),
            f"unix:path={SYSTEM_BUS}",
            json.dumps(trust_state.state("verified", trust_state.FROZEN)),
        ],
        stdout=subprocess.PIPE,
        text=True,
    )
    check(
        "an impostor of uid 65534 owns os.athanor.Update1",
        impostor.stdout.readline().strip() == "owned",
    )
    trust_state.write("verified")
    check(
        "its verified state names the shield 'Not verified yet'",
        wait_for(lambda: shield_named(app, Atspi, "Not verified yet"), 3),
    )
    check("and the sheet opens", toggle_sheet(app, Atspi, True))
    check("and says the state is not the system's", said(app, Atspi, UNTRUSTED))
    check("and closes", toggle_sheet(app, Atspi, False))
    impostor.terminate()
    impostor.wait()
    trust_state.write("verified")
    check(
        "root's answer is trusted again once the impostor leaves",
        wait_for(lambda: shield_named(app, Atspi, "System image verified"), 3),
    )

    # Hostile strings (Review Focus 2).
    trust_state.write("hostile")
    toggle_sheet(app, Atspi, True)
    shown = [
        text for text in labels(app, Atspi) or () if text.startswith("Version 43.")
    ]
    check("the hostile version is shown", len(shown) == 1, repr(shown))
    check(
        "without bidi or control characters",
        all("\u202e" not in t and "\u0007" not in t for t in shown),
    )
    check(
        "and within 128 characters of the file",
        all(len(t) <= len("Version ") + 128 for t in shown),
    )
    check(
        "the host is text, not markup",
        bool(labelled(app, Atspi, "label", "Host: <b>registry</b>.example")),
    )
    toggle_sheet(app, Atspi, False)

    # No update downloaded: no Restart to update in the power menu.
    trust_state.write("verified")
    press(app, Atspi, "Power")
    check(
        "without a download the power menu has no Restart to update",
        wait_for(lambda: bool(buttons(app, Atspi, "Restart")), 3)
        and not buttons(app, Atspi, "Restart to update"),
    )

    return 1 if failures else 0


if __name__ == "__main__":
    sys.exit(main())
