#!/usr/bin/env python3
"""
Test script: simulates Claude Code sending a PermissionRequest.
Run this while the Claude Keyboard app is running.

Usage:
    python3 test_permission.py [tool_name] [command]
    
Example:
    python3 test_permission.py Bash "rm -rf /tmp/test"
    python3 test_permission.py Write "write to /etc/hosts"
"""
import socket
import json
import sys

SOCKET_PATH = "/tmp/claude-keyboard.sock"

tool = sys.argv[1] if len(sys.argv) > 1 else "Bash"
command = sys.argv[2] if len(sys.argv) > 2 else "ls -la /Users/mapan"

event = {
    "session_id": "test-session-001",
    "cwd": "/Users/mapan/projects",
    "event": "PermissionRequest",
    "status": "waiting_for_approval",
    "pid": 12345,
    "tty": "/dev/ttys001",
    "tool": tool,
    "tool_input": {"command": command},
    "tool_use_id": "test-tool-001",
}

print(f"🔌 Connecting to {SOCKET_PATH}...")
sock = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
sock.settimeout(300)

try:
    sock.connect(SOCKET_PATH)
except ConnectionRefusedError:
    print("❌ Connection refused. Is Claude Keyboard app running?")
    sys.exit(1)
except FileNotFoundError:
    print("❌ Socket not found. Is Claude Keyboard app running?")
    sys.exit(1)

sock.sendall(json.dumps(event).encode())
sock.shutdown(socket.SHUT_WR)
print(f"📤 Sent PermissionRequest for tool: {tool}")
print(f"   command: {command}")
print(f"⏳ Waiting for your decision (click a button in the app)...")

response = b""
while True:
    chunk = sock.recv(4096)
    if not chunk:
        break
    response += chunk
sock.close()

if response:
    result = json.loads(response.decode())
    decision = result.get("decision", "unknown")
    emoji = {"allow": "✅", "deny": "❌"}.get(decision, "❓")
    print(f"\n{emoji} Decision: {decision}")
    if result.get("reason"):
        print(f"   Reason: {result['reason']}")
else:
    print("\n⚠️  No response received (socket closed)")
