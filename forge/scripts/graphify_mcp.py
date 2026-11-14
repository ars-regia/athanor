from mcp.server.fastmcp import FastMCP
import networkx as nx
import json
import os

# Initialize FastMCP Server
mcp = FastMCP("Graphify")

# Graph Database file
GRAPH_FILE = ".graphify/graphify_db.json"

def load_graph():
    G = nx.Graph()
    if os.path.exists(GRAPH_FILE):
        try:
            with open(GRAPH_FILE, 'r') as f:
                data = json.load(f)
                from networkx.readwrite import json_graph
                G = json_graph.node_link_graph(data)
        except Exception:
            pass
    return G

def save_graph(G):
    from networkx.readwrite import json_graph
    data = json_graph.node_link_data(G)
    os.makedirs(os.path.dirname(GRAPH_FILE), exist_ok=True)
    with open(GRAPH_FILE, 'w') as f:
        json.dump(data, f, indent=2)

@mcp.tool()
def query_graph(query: str) -> str:
    """Queries the graph for nodes and edges matching a specific term."""
    G = load_graph()
    results = []
    
    # Search nodes
    for node, data in G.nodes(data=True):
        if query.lower() in str(node).lower() or query.lower() in str(data).lower():
            results.append(f"Node: {node} | Attributes: {data}")
            
    # Search edges
    for u, v, data in G.edges(data=True):
        if query.lower() in str(u).lower() or query.lower() in str(v).lower() or query.lower() in str(data).lower():
            results.append(f"Edge: [{u}] <--> [{v}] | Relation: {data.get('relation', '')}")
            
    if not results:
        return f"No matches found for '{query}' in the Graphify database."
    return "Graphify Query Results:\n" + "\n".join(results)

@mcp.tool()
def shortest_path(source: str, target: str) -> str:
    """Finds the shortest topological path between two nodes in the system."""
    G = load_graph()
    try:
        path = nx.shortest_path(G, source=source, target=target)
        return f"Shortest path from '{source}' to '{target}':\n" + " -> ".join(path)
    except nx.NetworkXNoPath:
        return f"No path exists between '{source}' and '{target}'."
    except nx.NodeNotFound as e:
        return f"Node Error: {str(e)}"

@mcp.tool()
def add_node(name: str, description: str) -> str:
    """Adds a system component or node to the Graphify database."""
    G = load_graph()
    G.add_node(name, description=description)
    save_graph(G)
    return f"Node '{name}' successfully added to Graphify."

@mcp.tool()
def add_edge(source: str, target: str, relationship: str) -> str:
    """Adds a relationship (edge) between two nodes."""
    G = load_graph()
    if source not in G:
        G.add_node(source)
    if target not in G:
        G.add_node(target)
        
    G.add_edge(source, target, relation=relationship)
    save_graph(G)
    return f"Edge created: [{source}] --({relationship})--> [{target}]"

if __name__ == "__main__":
    mcp.run()
