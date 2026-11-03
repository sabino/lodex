#!/usr/bin/env python3
"""Hash, resume and verify a MIPS image in the isolated Omega lab; never cut over."""
import argparse
import gzip
import hashlib
import json
from pathlib import Path
import re
import shlex
import struct
import subprocess
import time


def ssh(helper, command, **kwargs):
    return subprocess.run([str(helper), command], check=True, **kwargs)


def prepare(image, cache, name):
    cache.mkdir(parents=True, exist_ok=True)
    compressed = cache / (name + ".gz")
    digest = hashlib.sha256()
    size = 0
    with image.open("rb") as source:
        header = source.read(52)
        if (len(header) != 52 or header[:6] != b"\x7fELF\x01\x01"
                or struct.unpack_from("<H", header, 18)[0] != 8
                or struct.unpack_from("<H", header, 16)[0] not in (2, 3)
                or struct.unpack_from("<I", header, 24)[0] == 0):
            raise ValueError("expected a 32-bit little-endian MIPS executable")
        source.seek(0)
        with compressed.open("wb") as output, gzip.GzipFile(
                filename="", mode="wb", fileobj=output, mtime=0) as zipper:
            while chunk := source.read(1024 * 1024):
                digest.update(chunk)
                size += len(chunk)
                zipper.write(chunk)
    with compressed.open("rb") as source:
        compressed_digest = hashlib.file_digest(source, "sha256").hexdigest()
    manifest = {"name": name, "bytes": size, "sha256": digest.hexdigest(),
                "gzip_bytes": compressed.stat().st_size, "gzip_sha256": compressed_digest}
    (cache / (name + ".manifest.json")).write_text(json.dumps(manifest, indent=2) + "\n")
    return compressed, manifest


def run(args):
    compressed, manifest = prepare(args.image, args.cache, args.name)
    q = shlex.quote
    root = args.lab_root.rstrip("/")
    payload = root + "/" + args.name + "." + manifest["gzip_sha256"][:16] + ".gz"
    target = root + "/bin/" + args.name
    ssh(args.ssh, f"set -eu\n[ -d {q(root)} ]\n[ -d {q(root + '/bin')} ]\n[ ! -e {q(target)} ]")
    for attempt in range(1, 4):
        result = ssh(args.ssh, f"if [ -f {q(payload)} ]; then wc -c < {q(payload)}; else echo 0; fi",
                     capture_output=True, text=True)
        offset = int(result.stdout.strip())
        if not 0 <= offset <= manifest["gzip_bytes"]:
            raise ValueError("remote partial payload has an invalid size")
        if offset == manifest["gzip_bytes"]:
            break
        print(f"upload {args.name}: attempt {attempt}, offset {offset}", flush=True)
        try:
            with compressed.open("rb") as source:
                source.seek(offset)
                ssh(args.ssh, f"umask 077\ncat >> {q(payload)}", stdin=source)
        except subprocess.CalledProcessError:
            if attempt == 3:
                raise
            time.sleep(2)
    # This runs asynchronously under the orchestration tool's exec session. Hashing
    # the large decoded ELF can take minutes on MIPS; do not restart on a poll timeout.
    temporary = target + "." + manifest["sha256"][:16] + ".new"
    command = f"""set -eu
umask 077
[ ! -e {q(target)} ]
[ "$(wc -c < {q(payload)})" -eq {manifest['gzip_bytes']} ]
[ "$(sha256sum {q(payload)} | cut -d ' ' -f 1)" = {q(manifest['gzip_sha256'])} ]
gzip -dc {q(payload)} > {q(temporary)}
[ "$(wc -c < {q(temporary)})" -eq {manifest['bytes']} ]
[ "$(sha256sum {q(temporary)} | cut -d ' ' -f 1)" = {q(manifest['sha256'])} ]
chmod 755 {q(temporary)}
mv {q(temporary)} {q(target)}
"""
    print(f"verify compressed and decoded image {args.name}", flush=True)
    ssh(args.ssh, command)
    print(json.dumps({**manifest, "installed_lab_image": target, "verified": True}), flush=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--ssh", required=True, type=Path)
    parser.add_argument("--image", required=True, type=Path)
    parser.add_argument("--name", required=True)
    parser.add_argument("--lab-root", default="/mnt/mmcblk0p2/lodex-lab")
    parser.add_argument("--cache", type=Path, default=Path(".lodex-build/uploads"))
    args = parser.parse_args()
    if not re.fullmatch(r"[a-zA-Z0-9][a-zA-Z0-9_.-]{0,63}", args.name):
        parser.error("name must be a simple bounded image name")
    if not Path(args.lab_root).is_absolute() or Path(args.lab_root).name != "lodex-lab":
        parser.error("lab root must be an absolute dedicated lodex-lab directory")
    run(args)


if __name__ == "__main__":
    main()
