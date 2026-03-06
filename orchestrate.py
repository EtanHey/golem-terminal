#!/usr/bin/env python3
"""Golem Terminal Orchestrator — programmatic control for orcClaude.

Launches the terminal GUI, opens tabs, starts Claude sessions, toggles
split-screen — all via the UDS control channel.

Usage:
    ./orchestrate.py                         # Interactive REPL
    ./orchestrate.py demo                    # Auto-demo: 2 tabs + split + screenshot
    ./orchestrate.py launch-claude           # Launch Claude in tab 0
    ./orchestrate.py multi <n>               # Open N tabs, each with Claude
    ./orchestrate.py screenshot [path]       # Take macOS screenshot
"""

import glob
import json
import os
import signal
import socket
import subprocess
import sys
import time


# ── Socket Communication ─────────────────────────────────────────────────────

def find_socket() -> str:
    """Return the path of the newest golem-terminal-debug-*.sock file."""
    candidates = glob.glob("/tmp/golem-terminal-debug-*.sock")
    if not candidates:
        return ""
    candidates.sort(key=lambda p: os.path.getmtime(p), reverse=True)
    return candidates[0]


def send_cmd(sock_path: str, payload: dict) -> dict:
    """Connect, send JSON command, read JSON response, close."""
    sock = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
    sock.settimeout(10)
    try:
        sock.connect(sock_path)
        msg = json.dumps(payload) + "\n"
        sock.sendall(msg.encode())
        buf = b""
        while True:
            chunk = sock.recv(4096)
            if not chunk:
                break
            buf += chunk
            if b"\n" in buf:
                break
    finally:
        sock.close()
    line = buf.decode(errors="replace").strip()
    if not line:
        return {}
    try:
        return json.loads(line)
    except json.JSONDecodeError:
        return {"raw": line}


def fire_and_forget(sock_path: str, payload: dict) -> None:
    """Send a command that has no response (launch, kill, etc.)."""
    sock = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
    sock.settimeout(5)
    try:
        sock.connect(sock_path)
        msg = json.dumps(payload) + "\n"
        sock.sendall(msg.encode())
        # Give the GUI a moment to process before closing.
        time.sleep(0.1)
    finally:
        sock.close()


# ── Helpers ───────────────────────────────────────────────────────────────────

def decode_hex(hex_str: str) -> str:
    """Decode a hex-encoded string to UTF-8 text."""
    try:
        return bytes.fromhex(hex_str).decode(errors="replace")
    except ValueError:
        return hex_str


def wait_for_socket(timeout: float = 10.0) -> str:
    """Wait for a golem-terminal debug socket to appear."""
    start = time.time()
    while time.time() - start < timeout:
        path = find_socket()
        if path:
            return path
        time.sleep(0.2)
    return ""


def wait_for_status(sock_path: str, slot: int, target: str, timeout: float = 30.0) -> bool:
    """Poll until slot reaches target status."""
    start = time.time()
    while time.time() - start < timeout:
        resp = send_cmd(sock_path, {"cmd": "status", "slot": slot})
        if resp.get("status") == target:
            return True
        time.sleep(0.2)
    return False


def take_screenshot(output_path: str = "/tmp/golem-terminal-screenshot.png") -> str:
    """Take a macOS screenshot of the frontmost window."""
    # Small delay to let UI settle.
    time.sleep(0.5)
    subprocess.run(["screencapture", "-l",
                     _get_window_id(), output_path],
                    capture_output=True)
    if os.path.exists(output_path):
        return output_path
    # Fallback: capture entire screen.
    subprocess.run(["screencapture", output_path], capture_output=True)
    return output_path if os.path.exists(output_path) else ""


def _get_window_id() -> str:
    """Get macOS window ID for Golem Terminal (hardcoded, safe)."""
    try:
        script = (
            'tell application "System Events" to get id of first window of '
            '(first process whose name is "Golem Terminal")'
        )
        result = subprocess.run(
            ["osascript", "-e", script],
            capture_output=True, text=True, timeout=5
        )
        return result.stdout.strip()
    except Exception:
        return "0"


# ── Launch Terminal ───────────────────────────────────────────────────────────

def launch_terminal(extra_args: list[str] | None = None) -> tuple[subprocess.Popen, str]:
    """Build and launch golem-terminal, return (process, socket_path)."""
    script_dir = os.path.dirname(os.path.abspath(__file__))
    cargo = os.environ.get("CARGO", os.path.expanduser("~/.cargo/bin/cargo"))

    print("Building golem-terminal...")
    subprocess.run(
        [cargo, "build", "--features", "gui", "--manifest-path",
         os.path.join(script_dir, "Cargo.toml")],
        check=True, capture_output=True
    )

    binary = os.path.join(
        os.environ.get("CARGO_TARGET_DIR", os.path.join(script_dir, "target")),
        "debug", "golem-terminal"
    )

    sock_path = f"/tmp/golem-terminal-debug-{os.getpid()}.sock"
    env = os.environ.copy()
    env["SESHAT_TEST_SOCKET"] = sock_path

    cmd = [binary, "ui", "--"]
    if extra_args:
        cmd.extend(extra_args)

    print(f"Launching: {' '.join(cmd)}")
    proc = subprocess.Popen(cmd, env=env)

    print("Waiting for socket...")
    found = wait_for_socket(timeout=15)
    if not found:
        proc.kill()
        raise RuntimeError("Socket never appeared")

    print(f"Connected: {found}")
    return proc, found


# ── Commands ──────────────────────────────────────────────────────────────────

def cmd_status(sock_path: str, slot: int = 0) -> None:
    """Print status of a slot."""
    resp = send_cmd(sock_path, {"cmd": "status", "slot": slot})
    print(f"Slot {slot}: {resp}")


def cmd_slot_count(sock_path: str) -> int:
    """Get number of tabs."""
    resp = send_cmd(sock_path, {"cmd": "slot_count"})
    count = resp.get("slot_count", 0)
    print(f"Tabs: {count}")
    return count


def cmd_new_tab(sock_path: str) -> int:
    """Create a new tab, return its index."""
    old_count = send_cmd(sock_path, {"cmd": "slot_count"}).get("slot_count", 1)
    fire_and_forget(sock_path, {"cmd": "new_tab"})
    time.sleep(0.3)
    new_count = send_cmd(sock_path, {"cmd": "slot_count"}).get("slot_count", 1)
    idx = new_count - 1
    print(f"New tab created: slot {idx} (total: {new_count})")
    return idx


def cmd_launch(sock_path: str, slot: int = 0) -> None:
    """Launch the default command in a slot."""
    fire_and_forget(sock_path, {"cmd": "launch", "slot": slot})
    print(f"Launched slot {slot}")


def cmd_send_input(sock_path: str, data: str, slot: int = 0) -> None:
    """Send text input to a slot's PTY."""
    fire_and_forget(sock_path, {"cmd": "send_input", "data": data, "slot": slot})
    print(f"Sent to slot {slot}: {repr(data)}")


def cmd_select_tab(sock_path: str, slot: int) -> None:
    """Select a tab by index."""
    fire_and_forget(sock_path, {"cmd": "select_tab", "slot": slot})
    print(f"Selected tab {slot}")


def cmd_toggle_split(sock_path: str) -> None:
    """Toggle split-screen mode."""
    fire_and_forget(sock_path, {"cmd": "toggle_split"})
    time.sleep(0.2)
    resp = send_cmd(sock_path, {"cmd": "split_status"})
    print(f"Split: {resp}")


def cmd_content(sock_path: str, slot: int = 0) -> str:
    """Get VT100-parsed content of a slot."""
    resp = send_cmd(sock_path, {"cmd": "content", "slot": slot})
    hex_str = resp.get("content", "")
    text = decode_hex(hex_str)
    return text


def cmd_output(sock_path: str, slot: int = 0) -> str:
    """Get raw output of a slot."""
    resp = send_cmd(sock_path, {"cmd": "output", "slot": slot})
    hex_str = resp.get("output", "")
    text = decode_hex(hex_str)
    return text


# ── Demo Scenarios ────────────────────────────────────────────────────────────

def demo_multi_claude(sock_path: str, n: int = 2) -> None:
    """Open N tabs, launch Claude in each, toggle split-screen."""
    print(f"\n=== Demo: {n} Claude tabs with split-screen ===\n")

    # Launch in tab 0.
    cmd_launch(sock_path, slot=0)
    print("Waiting for tab 0 to become ready...")
    if not wait_for_status(sock_path, 0, "ready", timeout=30):
        print("WARNING: tab 0 did not become ready in time")
        return

    # Create additional tabs and launch.
    for i in range(1, n):
        idx = cmd_new_tab(sock_path)
        cmd_launch(sock_path, slot=idx)
        print(f"Waiting for tab {idx} to become ready...")
        if not wait_for_status(sock_path, idx, "ready", timeout=30):
            print(f"WARNING: tab {idx} did not become ready in time")
            return

    # Toggle split-screen.
    if n >= 2:
        cmd_select_tab(sock_path, 0)
        time.sleep(0.3)
        cmd_toggle_split(sock_path)
        print("\nSplit-screen enabled! Tab 0 (left) + Tab 1 (right)")

    # Take screenshot.
    time.sleep(2)  # Let output render.
    path = take_screenshot()
    if path:
        print(f"\nScreenshot saved: {path}")

    print("\n=== Demo complete ===")


def demo_with_launch(n: int = 2) -> None:
    """Full demo: build, launch terminal, open N Claude tabs, split-screen."""
    proc, sock_path = launch_terminal(
        ["claude", "--dangerously-skip-permissions"]
    )
    try:
        demo_multi_claude(sock_path, n)
        print("\nTerminal is running. Press Ctrl+C to stop.")
        proc.wait()
    except KeyboardInterrupt:
        print("\nStopping...")
        fire_and_forget(sock_path, {"cmd": "quit"})
        proc.wait(timeout=5)
    finally:
        try:
            proc.kill()
        except ProcessLookupError:
            pass


# ── Interactive REPL ──────────────────────────────────────────────────────────

def run_interactive() -> None:
    """Interactive control of a running terminal instance."""
    sock_path = find_socket()
    if not sock_path:
        print("No running terminal found. Launch one first with:")
        print("  ./launch.sh")
        print("Or use: ./orchestrate.py demo")
        sys.exit(1)

    print(f"Connected to {sock_path}")
    print("Commands: status [slot], launch [slot], new_tab, select_tab <n>,")
    print("          toggle_split, split_status, content [slot], output [slot],")
    print("          send <text> [slot], kill [slot], slot_count, screenshot, quit")
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

        parts = line.split(None, 1)
        cmd = parts[0].lower()
        rest = parts[1] if len(parts) > 1 else ""

        try:
            if cmd == "status":
                slot = int(rest) if rest else 0
                cmd_status(sock_path, slot)
            elif cmd == "launch":
                slot = int(rest) if rest else 0
                cmd_launch(sock_path, slot)
            elif cmd == "new_tab":
                cmd_new_tab(sock_path)
            elif cmd == "select_tab":
                cmd_select_tab(sock_path, int(rest))
            elif cmd == "toggle_split":
                cmd_toggle_split(sock_path)
            elif cmd == "split_status":
                resp = send_cmd(sock_path, {"cmd": "split_status"})
                print(resp)
            elif cmd == "content":
                slot = int(rest) if rest else 0
                text = cmd_content(sock_path, slot)
                print(f"--- content ({len(text)} chars) ---")
                print(text[-2000:] if len(text) > 2000 else text)
            elif cmd == "output":
                slot = int(rest) if rest else 0
                text = cmd_output(sock_path, slot)
                print(f"--- output ({len(text)} chars) ---")
                print(text[-2000:] if len(text) > 2000 else text)
            elif cmd == "send":
                # Parse "text [slot]" — last word might be slot number.
                send_parts = rest.rsplit(None, 1)
                if len(send_parts) == 2 and send_parts[1].isdigit():
                    data = send_parts[0]
                    slot = int(send_parts[1])
                else:
                    data = rest
                    slot = 0
                # Safe escape: only \r and \n, not arbitrary \x sequences.
                data = data.replace("\\r", "\r").replace("\\n", "\n").replace("\\t", "\t")
                cmd_send_input(sock_path, data, slot)
            elif cmd == "kill":
                slot = int(rest) if rest else 0
                fire_and_forget(sock_path, {"cmd": "kill", "slot": slot})
                print(f"Killed slot {slot}")
            elif cmd == "slot_count":
                cmd_slot_count(sock_path)
            elif cmd == "active_tab":
                resp = send_cmd(sock_path, {"cmd": "active_tab"})
                print(resp)
            elif cmd == "screenshot":
                path = rest.strip() or "/tmp/golem-terminal-screenshot.png"
                result = take_screenshot(path)
                print(f"Screenshot: {result}" if result else "Screenshot failed")
            elif cmd == "quit":
                fire_and_forget(sock_path, {"cmd": "quit"})
                print("Sent quit")
                break
            elif cmd == "demo":
                n = int(rest) if rest else 2
                demo_multi_claude(sock_path, n)
            else:
                # Try raw command.
                payload = {"cmd": cmd}
                if rest and rest.isdigit():
                    payload["slot"] = int(rest)
                resp = send_cmd(sock_path, payload)
                print(resp)
        except Exception as e:
            print(f"Error: {e}")
        print()


# ── Main ──────────────────────────────────────────────────────────────────────

def main() -> None:
    args = sys.argv[1:]

    if not args:
        run_interactive()
    elif args[0] == "demo":
        n = int(args[1]) if len(args) > 1 else 2
        demo_with_launch(n)
    elif args[0] == "multi":
        n = int(args[1]) if len(args) > 1 else 2
        sock_path = find_socket()
        if not sock_path:
            print("No running terminal. Use 'demo' to launch one.")
            sys.exit(1)
        demo_multi_claude(sock_path, n)
    elif args[0] == "launch-claude":
        sock_path = find_socket()
        if not sock_path:
            print("No running terminal. Use ./launch.sh first.")
            sys.exit(1)
        cmd_launch(sock_path, 0)
    elif args[0] == "screenshot":
        path = args[1] if len(args) > 1 else "/tmp/golem-terminal-screenshot.png"
        result = take_screenshot(path)
        print(f"Screenshot: {result}" if result else "Screenshot failed")
    else:
        print(__doc__)


if __name__ == "__main__":
    main()
