"""nm_agent_template.py - python3-dbusmock's NetworkManager template with an agent manager,
for system_fixtures.py (doc_bar.md, BR9).

NetworkManager exports its agent manager before it owns its bus name, so a secret agent
registers as soon as the name appears. The upstream template has no agent manager, and one
added over D-Bus after the mock started would miss that first Register after a restart:
dbusmock owns the name before it loads the template. It dispatches no call until the template
is loaded, though, so an agent manager added here is in place for the first call.

Parameters, besides the upstream template's: "agent_manager", the object path; "fixture",
the interface of the fixture methods; "agent_methods" and "fixture_methods", lists of
(name, in signature, out signature, code) as AddMethods takes them; "held", a bus name
that holds the agent ID os.athanor.Bar from the start, or null. The agent methods see their
caller as _["sender"] and the held IDs as self.agents, {(uid, ID): bus name}.
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
    held = parameters.pop("held")
    upstream.load(mock, parameters)
    mock.AddObject(agent_manager, AGENT_MANAGER_IFACE, {}, agent_methods)
    manager = dbusmock.get_object(agent_manager)
    manager.AddMethods(fixture, fixture_methods)
    # dbus-python passes the caller's unique name to a method under this keyword.
    for name, *_ in agent_methods:
        manager.methods[AGENT_MANAGER_IFACE][name][3]._dbus_sender_keyword = "sender"
    manager.agents = {}
    if held is not None:
        manager.agents[(manager.connection.get_unix_user(held), "os.athanor.Bar")] = held
