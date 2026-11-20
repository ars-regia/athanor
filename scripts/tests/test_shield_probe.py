"""Unit tests of the shield sheet probe of the VM acceptance, on a fake AT-SPI tree
(python3 -B -m unittest discover -s scripts/tests -v)."""

import enum
import importlib.util
import pathlib
import sys
import types
import unittest
from unittest import mock

ROOT = pathlib.Path(__file__).resolve().parents[2]


class Role(enum.Enum):
    PUSH_BUTTON = 1
    LABEL = 2
    TOGGLE_BUTTON = 3
    CHECK_BOX = 4
    SWITCH = 5
    PANEL = 6


class StateType(enum.Enum):
    SHOWING = 1


class GLibError(Exception):
    pass


Atspi = types.SimpleNamespace(Role=Role, StateType=StateType)
gi = types.ModuleType("gi")
gi.require_version = lambda *_: None
repository = types.ModuleType("gi.repository")
repository.Atspi = Atspi
repository.GLib = types.SimpleNamespace(Error=GLibError)
gi.repository = repository

with mock.patch.dict(sys.modules, {"gi": gi, "gi.repository": repository}):
    spec = importlib.util.spec_from_file_location(
        "shield_probe", ROOT / "scripts" / "devvm" / "shield_probe.py"
    )
    probe = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(probe)


class Node:
    def __init__(self, role, name="", showing=True, children=()):
        self.role, self.name, self.showing, self.children = (
            role,
            name,
            showing,
            children,
        )

    def get_role(self):
        return self.role

    def get_name(self):
        return self.name

    def get_state_set(self):
        return types.SimpleNamespace(contains=lambda _: self.showing)

    def get_child_count(self):
        return len(self.children)

    def get_child_at_index(self, i):
        return self.children[i]


NAME = {"Automatic security updates"}


class FindTests(unittest.TestCase):
    def test_a_switch_is_found_by_its_own_name(self):
        for role in (Role.TOGGLE_BUTTON, Role.CHECK_BOX, Role.SWITCH):
            switch = Node(role, "Automatic security updates")
            app = Node(Role.PANEL, children=(switch,))
            self.assertIs(probe.find(app, NAME, probe.ROLES), switch, role)

    def test_a_switch_that_is_not_showing_is_not_found(self):
        app = Node(
            Role.PANEL,
            children=(
                Node(Role.TOGGLE_BUTTON, "Automatic security updates", showing=False),
            ),
        )
        self.assertIsNone(probe.find(app, NAME, probe.ROLES))


if __name__ == "__main__":
    unittest.main()
