#!/usr/bin/env python3
"""Debug helper for Golem Terminal's UDS test/control channel.

Finds the newest /tmp/golem-terminal-debug-*.sock and connects to it.

Usage:
    ./debug.sh                  # interactive REPL
    ./debug.sh status           # single query (slot 0)
    ./debug.sh content          # decoded VT content
    ./debug.sh output           # decoded raw output
    ./debug.sh send "hello\\r"  # send input to the child PTY
    ./debug.sh status 1         # query slot 1
"""

import glob
import json
import os
import readline  # noqa: F401 — enables line editing in input()
import socket
import sys


def find_socket() -> str:
    """Return the path of the newest golem-terminal-debug-*.sock file."""
    candidates = glob.glob("/tmp/golem-terminal-debug-*.sock")
    if not candidates:
        sys.exit("No /tmp/golem-terminal-debug-*.sock found. Is Golem Terminal running with launch.sh?")
    # Sort by modification time, newest first.
    candidates.sort(key=lambda p: os.path.getmtime(p), reverse=True)
    return candidates[0]


def connect(path: str) -> socket.socket:
    """Connect to the given Unix domain socket."""
    sock = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
    try:
        sock.connect(path)
    except (ConnectionRefusedError, FileNotFoundError) as exc:
        sys.exit(f"Cannot connect to {path}: {exc}")
    return sock


def send_cmd(sock: socket.socket, payload: str) -> str:
    """Send a newline-terminated JSON command and read back one line."""
    sock.sendall((payload.rstrip("\n") + "\n").encode())
    # Read until newline.
    buf = b""
    while True:
        chunk = sock.recv(4096)
        if not chunk:
            break
        buf += chunk
        if b"\n" in buf:
            break
    return buf.decode(errors="replace").strip()


def decode_hex(hex_str: str) -> str:
    """Decode a hex-encoded string to UTF-8 text."""
    try:
        return bytes.fromhex(hex_str).decode(errors="replace")
    except ValueError:
        return hex_str


def format_size(n: int) -> str:
    """Human-readable byte count."""
    if n < 1024:
        return f"{n} B"
    elif n < 1024 * 1024:
        return f"{n / 1024:.1f} KB"
    else:
        return f"{n / (1024 * 1024):.1f} MB"


def format_response(cmd: str, raw: str) -> str:
    """Pretty-print a JSON response, decoding hex fields."""
    if not raw:
        return "(no response -- command was fire-and-forget)"
    try:
        obj = json.loads(raw)
    except json.JSONDecodeError:
        return raw

    # Decode hex-encoded fields.
    for key in ("output", "content"):
        if key in obj:
            decoded = decode_hex(obj[key])
            byte_count = len(obj[key]) // 2
            header = f"--- {key} ({format_size(byte_count)}) ---"
            return f"{header}\n{decoded}"

    # status and other simple responses.
    return json.dumps(obj, indent=2)


def build_payload(args: list[str]) -> str:
    """Turn CLI args into a JSON command string."""
    if not args:
        return ""
    cmd = args[0].lower()
    slot = 0

    # Check for trailing slot number.
    if len(args) >= 2 and args[-1].isdigit() and cmd != "send":
        slot = int(args[-1])

    if cmd == "send":
        data = args[1] if len(args) > 1 else ""
        # Process escape sequences so "hello\r" works.
        data = data.encode().decode("unicode_escape")
        return json.dumps({"cmd": "send_input", "data": data, "slot": slot})
    elif cmd in ("status", "output", "content", "launch", "kill", "quit"):
        return json.dumps({"cmd": cmd, "slot": slot})
    else:
        return json.dumps({"cmd": cmd, "slot": slot})


def run_interactive(sock_path: str) -> None:
    """Interactive REPL -- reconnects per command."""
    print(f"Connected to {sock_path}")
    print("Commands: status, output, content, send <text>, launch, kill, quit")
    print("Append slot number for non-default slot (e.g. 'status 1')")
    print()
    while True:
        try:
            line = input("golem> ").strip()
        except (EOFError, KeyboardInterrupt):
            print()
            break
        if not line:
            continue
        if line.lower() in ("exit", "q"):
            break

        payload = build_payload(line.split())
        if not payload:
            continue

        # Reconnect each time (harness accepts multiple connections).
        try:
            sock = connect(sock_path)
            raw = send_cmd(sock, payload)
            sock.close()
        except (BrokenPipeError, ConnectionResetError) as exc:
            print(f"Connection error: {exc}")
            continue

        print(format_response(line.split()[0].lower(), raw))
        print()


def run_oneshot(sock_path: str, args: list[str]) -> None:
    """Send a single command, print result, exit."""
    payload = build_payload(args)
    if not payload:
        sys.exit("Unknown command")

    sock = connect(sock_path)
    raw = send_cmd(sock, payload)
    sock.close()

    cmd = args[0].lower()
    print(format_response(cmd, raw))


def main() -> None:
    sock_path = find_socket()
    args = sys.argv[1:]

    if args:
        run_oneshot(sock_path, args)
    else:
        run_interactive(sock_path)


if __name__ == "__main__":
    main()
