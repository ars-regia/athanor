"""Static checks of system/Containerfile: no state of the build itself reaches a published layer, so
two builds of the same tree write the same layers and an upgrade downloads only what changed
(doc_update_delivery.md, UD34; #146). system/build-image.sh adds --timestamp, which
test_build_image.py checks. (python3 -B -m unittest discover -s system/tests -v)"""

import os
import pathlib
import re
import unittest

ROOT = pathlib.Path(__file__).resolve().parents[2]
CONTAINERFILE = pathlib.Path(
    os.environ.get("CONTAINERFILE", ROOT / "system" / "Containerfile")
)
# What dnf5 and ldconfig write that no package owns and that differs on every build (measured
# between runs 38388982529 and 38540020655 of the same tree).
STATE = (
    "/usr/lib/sysimage/libdnf5/transaction_history.sqlite",
    "/var/log/dnf5.log",
    "/var/cache/ldconfig/aux-cache",
)
# Commands that write it, or install packages and so record install times.
WRITERS = re.compile(
    r"\bdnf5 +(install|remove|clean)\b|\bldconfig\b|initramfs\.sh|\brpm +-[UiFe]"
)
CLEAR = "rm -f $BUILD_STATE"
# Commands that stamp a time into what reaches an image: rpm's install times, the /etc/shadow
# last-change field, the font cache checksums, an RPM's build and file times, the initramfs.
STAMPERS = re.compile(
    r"\bdnf5 +install\b|\brpm +-U|\brpmbuild\b|build-rpms\.sh|initramfs\.sh"
)
# The scripts those stages run, which must take the epoch of the Containerfile and keep none of
# their own.
SCRIPTS = (
    ROOT / "system" / "scripts" / "initramfs.sh",
    ROOT / "system" / "nvidia" / "build-rpms.sh",
)


def stages(text):
    """[(stage name, [instruction, ...])], the global ARGs first under the name ''. Continuation
    lines are joined; comment lines are dropped, as the Containerfile parser drops them."""
    result, current, pending = [("", [])], [], ""
    for line in text.splitlines():
        if line.lstrip().startswith("#") or not line.strip():
            continue
        pending += line.rstrip()
        if pending.endswith("\\"):
            pending = pending[:-1] + " "
            continue
        instruction, pending = " ".join(pending.split()), ""
        if instruction.startswith("FROM "):
            result.append((instruction.split()[-1], []))
        result[-1][1].append(instruction)
    return result


class BuildState(unittest.TestCase):
    def setUp(self):
        text = CONTAINERFILE.read_text()
        self.stages = stages(text)
        self.by_name = dict(self.stages)
        self.copied_from = set(re.findall(r"(?:--|,)from=([\w-]+)", text))

    def layered(self):
        """The stages whose own layers reach an image; a stage read only through COPY --from or
        a mount's from= gives the image just the paths read from it."""
        return [
            (name, body)
            for name, body in self.stages[1:]
            if name not in self.copied_from
        ]

    def test_the_epoch_and_the_state_are_declared_for_every_stage(self):
        glob = self.by_name[""]
        self.assertTrue(
            any(re.fullmatch(r"ARG SOURCE_DATE_EPOCH=[0-9]+", i) for i in glob),
            "no global ARG SOURCE_DATE_EPOCH=<seconds>",
        )
        [state] = [i for i in glob if i.startswith("ARG BUILD_STATE=")] or [""]
        for path in STATE:
            self.assertIn(path, state, f"ARG BUILD_STATE does not name {path}")

    def test_every_stage_that_stamps_a_time_has_the_epoch_first(self):
        for name, body in self.stages[1:]:
            runs = [
                n
                for n, i in enumerate(body)
                if i.startswith("RUN ") and STAMPERS.search(i)
            ]
            if runs:
                arg = [n for n, i in enumerate(body) if i == "ARG SOURCE_DATE_EPOCH"]
                self.assertTrue(
                    arg and arg[0] < runs[0],
                    f"stage {name} stamps a time without ARG SOURCE_DATE_EPOCH first",
                )

    def test_the_scripts_take_the_epoch_of_the_containerfile(self):
        for script in SCRIPTS:
            text = script.read_text()
            self.assertIn(
                "${SOURCE_DATE_EPOCH:?",
                text,
                f"{script.name} does not require the epoch",
            )
            self.assertNotRegex(
                text,
                r"SOURCE_DATE_EPOCH(:?-|=[0-9])",
                f"{script.name} keeps an epoch of its own",
            )

    def test_every_rpm_built_for_an_image_carries_the_epoch(self):
        # rpmbuild of Fedora takes the build time from the clock, and the epoch from the spec's
        # changelog, unless told otherwise; the rpm database records both.
        text = (ROOT / "system" / "nvidia" / "build-rpms.sh").read_text()
        for macro in (
            "source_date_epoch_from_changelog 0",
            "use_source_date_epoch_as_buildtime 1",
            "clamp_mtime_to_source_date_epoch 1",
            "_buildhost athanor-build",
        ):
            self.assertIn(f'--define "{macro}"', text)
        calls = re.findall(r"^\s*rpmbuild\b.*$", text, re.M)
        self.assertTrue(calls, "build-rpms.sh runs no rpmbuild")
        for call in calls:
            self.assertIn('"${epoch[@]}"', call, f"{call.strip()[:80]}...")

    def test_the_system_stage_ends_by_clearing_the_state_before_the_rechunk(self):
        body = self.by_name["system"]
        last = [i for i in body if i.startswith("RUN ")][-1]
        self.assertTrue(
            "ARG BUILD_STATE" in body, "the system stage uses no ARG BUILD_STATE"
        )
        self.assertIn(
            CLEAR, last, "the last RUN of the system stage does not clear $BUILD_STATE"
        )
        self.assertRegex(
            last,
            r"rm -rf /var/cache/libdnf5\b",
            "the last RUN of the system stage keeps the dnf5 metadata",
        )

    def test_every_later_run_clears_the_state_it_writes(self):
        # The stages after the rechunk are not regrouped: a layer keeps what its own RUN wrote.
        for name, body in self.layered():
            if name == "system":
                continue
            for run in (i for i in body if i.startswith("RUN ")):
                writers = [m.end() for m in WRITERS.finditer(run)]
                if writers:
                    self.assertTrue(
                        "ARG BUILD_STATE" in body,
                        f"stage {name} uses no ARG BUILD_STATE",
                    )
                    self.assertGreater(
                        run.rfind(CLEAR),
                        max(writers),
                        f"stage {name}: {run[:90]}... keeps its build state",
                    )


if __name__ == "__main__":
    unittest.main()
