"""nm_agent_template.py - python3-dbusmock's NetworkManager template with an agent manager,
for system_fixtures.py (doc_bar.md, BR9).

NetworkManager exports its agent manager before it owns its bus name, so a secret agent
registers as soon as the name appears. The upstream template has no agent manager, and one
added over D-Bus after the mock started would miss that first Register after a restart:
dbusmock owns the name before it loads the template. It dispatches no call until the template
is loaded, though, so an agent manager added here is in place for the first call.

Parameters, besides the upstream template's: "agent_manager", the object path; "fixture",
the interface of the fixture methods; "agent_methods" and "fixture_methods", lists of
(name, in signature, out signature, code) as AddMethods takes them.
"""

import dbusmock
from dbusmock.templates import networkmanager as upstream
from dbusmock.templates.networkmanager import *  # noqa: F401,F403 - the template's names and mock methods

AGENT_MANAGER_IFACE = "org.freedesktop.NetworkManager.AgentManager"


def load(mock, parameters):
    agent_manager = parameters.pop("agent_manager")
    fixture = parameters.pop("fixture")
    agent_methods = [tuple(method) for method in parameters.pop("agent_methods")]
    fixture_methods = [tuple(method) for method in parameters.pop("fixture_methods")]
    upstream.load(mock, parameters)
    mock.AddObject(agent_manager, AGENT_MANAGER_IFACE, {}, agent_methods)
    dbusmock.get_object(agent_manager).AddMethods(fixture, fixture_methods)
