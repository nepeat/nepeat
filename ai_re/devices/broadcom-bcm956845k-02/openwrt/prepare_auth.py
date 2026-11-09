#!/usr/bin/env python3
"""Keep a local, untracked management password across RAM-image rebuilds."""
import os
from pathlib import Path
import secrets
import subprocess


def prepare_auth(source: Path, work: Path):
    password_file = work / "luci-root-password.txt"
    if not password_file.exists():
        descriptor = os.open(password_file, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
        with os.fdopen(descriptor, "w") as stream:
            stream.write(secrets.token_urlsafe(24) + "\n")
    password_file.chmod(0o600)
    password = password_file.read_text().strip()
    if not password:
        raise SystemExit("Local management password file is empty")
    # Reuse the generated shadow file to avoid changing the image on every
    # build solely because SHA-512 crypt uses a random salt.
    shadow_file = work / "luci-shadow"
    if not shadow_file.exists():
        hashed = subprocess.run(["openssl", "passwd", "-6", "-stdin"],
                                input=password + "\n", text=True,
                                capture_output=True, check=True).stdout.strip()
        base = (source / "package/base-files/files/etc/shadow").read_text()
        lines = base.splitlines()
        for index, line in enumerate(lines):
            if line.startswith("root:"):
                fields = line.split(":")
                fields[1] = hashed
                lines[index] = ":".join(fields)
                break
        else:
            raise SystemExit("No root entry in base shadow file")
        descriptor = os.open(shadow_file, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
        with os.fdopen(descriptor, "w") as stream:
            stream.write("\n".join(lines) + "\n")
    destination = source / "files/etc/shadow"
    destination.parent.mkdir(parents=True, exist_ok=True)
    descriptor = os.open(destination, os.O_WRONLY | os.O_CREAT | os.O_TRUNC, 0o600)
    with os.fdopen(descriptor, "w") as stream:
        stream.write(shadow_file.read_text())
    destination.chmod(0o600)
    public_key = work / "management_key.pub"
    if public_key.exists():
        authorized = source / "files/etc/dropbear/authorized_keys"
        authorized.parent.mkdir(parents=True, exist_ok=True)
        authorized.write_text(public_key.read_text())
        authorized.chmod(0o600)
    host_key = work / "dropbear_ed25519_host_key"
    if host_key.exists():
        destination = source / "files/etc/dropbear/dropbear_ed25519_host_key"
        destination.parent.mkdir(parents=True, exist_ok=True)
        destination.write_bytes(host_key.read_bytes())
        destination.chmod(0o600)


if __name__ == "__main__":
    root = Path(__file__).resolve().parent
    prepare_auth(root / "work/source", root / "work")
