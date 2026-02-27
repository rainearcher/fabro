#!/usr/bin/env python3
"""Boot a Daytona sandbox, clone this repo, and run cargo build.

Usage:
    python3 daytona_build.py setup     # One-time: create a snapshot with Rust + deps
    python3 daytona_build.py build     # Create sandbox from snapshot, clone, build
    python3 daytona_build.py teardown  # Delete sandbox and snapshot

No external dependencies — uses only the Python standard library.
"""

import json
import os
import subprocess
import sys
import time
import urllib.request
import urllib.error

API_BASE = "https://app.daytona.io/api"
TOOLBOX_BASE = "https://proxy.app.daytona.io/toolbox"
REPO_URL = "https://github.com/brynary/attractor-rust.git"
REPO_BRANCH = "main"
REPO_PATH = "/home/daytona/attractor-rust"

SNAPSHOT_NAME = "attractor-rust-dev"
SANDBOX_NAME = "attractor-rust"

# Bake Rust + system deps + rustup update into the snapshot so sandbox
# creation is instant and no setup steps are needed at runtime.
DOCKERFILE = """\
FROM rust:1.85-slim-bookworm
RUN apt-get update && apt-get install -y --no-install-recommends \
        pkg-config libssl-dev cmake git curl ca-certificates \
    && rm -rf /var/lib/apt/lists/*
RUN rustup update stable && rustup default stable
RUN useradd -m -s /bin/bash daytona
USER daytona
WORKDIR /home/daytona
"""


def load_api_key() -> str:
    key = os.environ.get("DAYTONA_API_KEY")
    if key:
        return key

    env_path = os.path.join(os.path.dirname(os.path.abspath(__file__)), ".env")
    if os.path.exists(env_path):
        with open(env_path) as f:
            for line in f:
                line = line.strip()
                if line.startswith("export "):
                    line = line[len("export ") :]
                if line.startswith("DAYTONA_API_KEY="):
                    return line.split("=", 1)[1]

    print("Error: DAYTONA_API_KEY not found in environment or .env file")
    sys.exit(1)


def get_github_token() -> str:
    result = subprocess.run(
        ["gh", "auth", "token"], capture_output=True, text=True, check=True
    )
    return result.stdout.strip()


def api_request(
    url: str,
    headers: dict[str, str],
    method: str = "GET",
    body: dict | None = None,
    allow_errors: bool = False,
) -> dict | None:
    data = json.dumps(body).encode() if body is not None else None
    req = urllib.request.Request(url, data=data, headers=headers, method=method)
    try:
        with urllib.request.urlopen(req, timeout=660) as resp:
            raw = resp.read()
            if not raw:
                return None
            return json.loads(raw)
    except urllib.error.HTTPError as e:
        if allow_errors:
            return {"_error": e.code, "_body": e.read().decode()}
        error_body = e.read().decode()
        print(f"  HTTP {e.code}: {error_body}")
        sys.exit(1)


class DaytonaClient:
    def __init__(self, api_key: str):
        self.headers = {
            "Authorization": f"Bearer {api_key}",
            "Content-Type": "application/json",
        }

    # ── Snapshot management ──────────────────────────────────────────

    def get_snapshot(self, name: str) -> dict | None:
        result = api_request(
            f"{API_BASE}/snapshots/{name}", self.headers, allow_errors=True
        )
        if result and "_error" in result:
            return None
        return result

    def create_snapshot(self) -> dict:
        print(f"Creating snapshot '{SNAPSHOT_NAME}'...")
        data = api_request(
            f"{API_BASE}/snapshots",
            self.headers,
            method="POST",
            body={
                "name": SNAPSHOT_NAME,
                "buildInfo": {"dockerfileContent": DOCKERFILE},
                "cpu": 4,
                "memory": 8,
                "disk": 10,
            },
        )
        assert data is not None
        print(f"  State: {data.get('state', 'unknown')}")
        return data

    def wait_snapshot_ready(self, timeout: int = 600, interval: int = 10) -> None:
        print("Waiting for snapshot to build...")
        deadline = time.time() + timeout
        while time.time() < deadline:
            snap = self.get_snapshot(SNAPSHOT_NAME)
            if snap is None:
                print("  Snapshot disappeared!")
                sys.exit(1)
            state = snap.get("state", "unknown")
            print(f"  State: {state}")
            if state == "active":
                return
            if state in ("error", "failed"):
                print(f"  Snapshot failed: {snap.get('errorReason')}")
                sys.exit(1)
            time.sleep(interval)
        print("  Timed out waiting for snapshot")
        sys.exit(1)

    def activate_snapshot(self) -> None:
        print(f"Activating snapshot '{SNAPSHOT_NAME}'...")
        api_request(
            f"{API_BASE}/snapshots/{SNAPSHOT_NAME}/activate",
            self.headers,
            method="POST",
        )

    def delete_snapshot(self) -> None:
        print(f"Deleting snapshot '{SNAPSHOT_NAME}'...")
        api_request(
            f"{API_BASE}/snapshots/{SNAPSHOT_NAME}",
            self.headers,
            method="DELETE",
            allow_errors=True,
        )
        print("  Deleted.")

    # ── Sandbox management ───────────────────────────────────────────

    def create_sandbox(self) -> str:
        print(f"Creating sandbox from snapshot '{SNAPSHOT_NAME}'...")
        data = api_request(
            f"{API_BASE}/sandbox",
            self.headers,
            method="POST",
            body={
                "name": SANDBOX_NAME,
                "snapshot": SNAPSHOT_NAME,
                "autoStopInterval": 60,
                "labels": {"project": "attractor-rust"},
            },
        )
        assert data is not None
        sandbox_id = data["id"]
        print(f"  ID:    {sandbox_id}")
        print(f"  State: {data['state']}")
        print(f"  CPU: {data['cpu']}  Memory: {data['memory']} GB  Disk: {data['disk']} GB")
        return sandbox_id

    def wait_sandbox_started(
        self, sandbox_id: str, timeout: int = 600, interval: int = 5
    ) -> None:
        print("Waiting for sandbox to start...")
        deadline = time.time() + timeout
        while time.time() < deadline:
            data = api_request(
                f"{API_BASE}/sandbox/{sandbox_id}", self.headers
            )
            assert data is not None
            state = data["state"]
            print(f"  State: {state}")
            if state == "started":
                return
            if state in ("error", "build_failed"):
                reason = data.get("errorReason", "unknown")
                print(f"  Error: {reason}")
                sys.exit(1)
            time.sleep(interval)
        print("  Timed out waiting for sandbox to start")
        sys.exit(1)

    def delete_sandbox(self) -> None:
        print(f"Deleting sandbox '{SANDBOX_NAME}'...")
        api_request(
            f"{API_BASE}/sandbox/{SANDBOX_NAME}",
            self.headers,
            method="DELETE",
            allow_errors=True,
        )
        print("  Deleted.")

    def exec(
        self, sandbox_id: str, command: str, timeout_ms: int = 600_000
    ) -> dict:
        payload = {"command": f"bash -c {json.dumps(command)}", "timeout": timeout_ms}
        data = api_request(
            f"{TOOLBOX_BASE}/{sandbox_id}/process/execute",
            self.headers,
            method="POST",
            body=payload,
        )
        assert data is not None
        return data

    def exec_print(
        self, sandbox_id: str, label: str, command: str, timeout_ms: int = 600_000
    ) -> dict:
        print(f"{label}...")
        result = self.exec(sandbox_id, command, timeout_ms)
        exit_code = result.get("exitCode", -1)
        output = result.get("result", "")
        if output:
            for line in output.rstrip("\n").split("\n"):
                print(f"  {line}")
        if exit_code != 0:
            print(f"  Exit code: {exit_code}")
            sys.exit(1)
        return result

    def git_clone(self, sandbox_id: str, github_token: str) -> None:
        print(f"Cloning {REPO_URL} (branch: {REPO_BRANCH})...")
        api_request(
            f"{TOOLBOX_BASE}/{sandbox_id}/git/clone",
            self.headers,
            method="POST",
            body={
                "url": REPO_URL,
                "path": REPO_PATH,
                "branch": REPO_BRANCH,
                "username": "git",
                "password": github_token,
            },
        )
        print("  Clone complete.")


# ── Commands ─────────────────────────────────────────────────────────


def cmd_setup(client: DaytonaClient) -> None:
    """Create the snapshot (one-time)."""
    existing = client.get_snapshot(SNAPSHOT_NAME)
    if existing:
        state = existing.get("state", "unknown")
        if state == "active":
            print(f"Snapshot '{SNAPSHOT_NAME}' already exists and is active.")
            return
        if state == "inactive":
            client.activate_snapshot()
            client.wait_snapshot_ready()
            return
        print(f"Snapshot in state '{state}', deleting and recreating...")
        client.delete_snapshot()
        time.sleep(5)

    client.create_snapshot()
    client.wait_snapshot_ready()
    print(f"\nSnapshot '{SNAPSHOT_NAME}' is ready.")


def cmd_build(client: DaytonaClient) -> None:
    """Create a sandbox from the snapshot, clone, and build."""
    snap = client.get_snapshot(SNAPSHOT_NAME)
    if not snap or snap.get("state") != "active":
        print(f"Snapshot '{SNAPSHOT_NAME}' not found or not active. Run 'setup' first.")
        sys.exit(1)

    github_token = get_github_token()
    sandbox_id = client.create_sandbox()
    client.wait_sandbox_started(sandbox_id)

    client.exec_print(
        sandbox_id, "Checking Rust version", "rustc --version && cargo --version"
    )

    client.git_clone(sandbox_id, github_token)
    client.exec_print(
        sandbox_id,
        "Verifying repo",
        f"ls {REPO_PATH}/Cargo.toml && git -C {REPO_PATH} log --oneline -3",
    )

    client.exec_print(
        sandbox_id,
        "Running cargo build",
        f"cd {REPO_PATH} && cargo build 2>&1",
        timeout_ms=600_000,
    )

    print(f"\nDone! Sandbox {sandbox_id} is ready.")
    print(f"  Repo at: {REPO_PATH}")


def cmd_teardown(client: DaytonaClient) -> None:
    """Delete sandbox and snapshot."""
    client.delete_sandbox()
    client.delete_snapshot()
    print("Teardown complete.")


COMMANDS = {
    "setup": cmd_setup,
    "build": cmd_build,
    "teardown": cmd_teardown,
}


def main() -> None:
    cmd = sys.argv[1] if len(sys.argv) > 1 else "build"
    if cmd in ("-h", "--help"):
        print(__doc__)
        sys.exit(0)
    if cmd not in COMMANDS:
        print(f"Unknown command: {cmd}")
        print(f"Available: {', '.join(COMMANDS)}")
        sys.exit(1)

    api_key = load_api_key()
    client = DaytonaClient(api_key)
    COMMANDS[cmd](client)


if __name__ == "__main__":
    main()
