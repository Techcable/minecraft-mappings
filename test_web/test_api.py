import requests
import os

TARGETS = [
    "spigot2srg",
    "spigot2srg-onlyobf",
    "spigot2mcp",
    "obf2mcp",
    "mcp2obf"
]
BASE_URL = "http://localhost:8000"
MCP_VERSION = "snapshot_nodoc_20180925"
MINECRAFT_VERSION = "1.13"


def main():
    request = {
        "minecraft_version": MINECRAFT_VERSION,
        "mcp_version": MCP_VERSION,
        "targets": TARGETS
    }
    r = requests.post(f"{BASE_URL}/api/beta/load_mappings", json=request).json()
    print(f"Response keys {set(r.keys())}")
    response_time = r['response_time']
    serialized_mappings = r['serialized_mappings']
    print(f"Received {len(serialized_mappings)} mappings in {response_time}ms")
    os.makedirs("out", exist_ok=True),
    for target, serialized in serialized_mappings.items():
        with open(f"out/{target}-{MINECRAFT_VERSION}.srg", 'w', encoding='utf-8') as f:
            f.write(serialized)


if __name__ == "__main__":
    main()
