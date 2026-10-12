#!/usr/bin/env python3
"""Restore the path-free PHP catalog toolchain under ROOT, verifying each step against the lock.

    restore_environment.py [--root ROOT] [--verify-only]

Steps, in order: disk budget; the Debian packages bison, re2c and Z3, verified by SHA-256 and
extracted under ROOT/deps (nothing is installed system-wide); the php-src clone at the locked
commit; the SVF clone at the locked commit; the SVF build (build_svf_pathfree.sh); the php CLI
build and the CALL-mode php CLI build (build_php_pathfree.sh). A build is skipped when its
output already matches the lock. Any mismatch stops the run and names the value.

--verify-only checks what is present under ROOT against the lock and builds nothing.
"""

import argparse
import hashlib
import json
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

from run import resolve_record


HERE = Path(__file__).resolve().parent
LOCK = json.loads((HERE / "results" / "lock.json").read_text())
# Measured after a full restore: svf-pathfree 141 MiB, php-cli 341 MiB, php-call 335 MiB,
# deps 40 MiB, plus the php-src clone and the build copies; about 1.5 GiB in total.
REQUIRED_FREE_BYTES = 4 * 1024 ** 3
PHP_REPOSITORY = "https://github.com/php/php-src.git"
SVF_REPOSITORY = "https://github.com/SVF-tools/SVF.git"


class RestoreError(RuntimeError):
    pass


def sha(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def require(condition: bool, message: str) -> None:
    if not condition:
        raise RestoreError(message)


def check_disk(root: Path) -> None:
    free = shutil.disk_usage(root).free
    require(free >= REQUIRED_FREE_BYTES,
            f"{free // 1024 ** 2} MiB free under {root}; the restore needs "
            f"{REQUIRED_FREE_BYTES // 1024 ** 2} MiB")


def check_packages(root: Path, verify_only: bool) -> None:
    pinned = LOCK["downloaded_packages_sha256"]
    missing = [name for name in pinned if not (root / name).is_file()]
    if missing and not verify_only:
        subprocess.run(["apt-get", "download", "bison", "re2c", "libz3-dev", "libz3-4"],
                       cwd=root, check=True)
    for name, digest in pinned.items():
        path = root / name
        require(path.is_file(), f"package not present: {path}")
        require(sha(path) == digest, f"package digest differs from the lock: {name}")
    marker = root / "deps" / ".extracted"
    if not marker.is_file():
        require(not verify_only, "packages are not extracted; run without --verify-only")
        (root / "deps").mkdir(exist_ok=True)
        for name in pinned:
            subprocess.run(["dpkg-deb", "-x", str(root / name), str(root / "deps")], check=True)
        marker.write_text("\n".join(pinned) + "\n")


def check_clone(path: Path, url: str, commit: str, verify_only: bool, extra: list[str]) -> None:
    if not path.is_dir():
        require(not verify_only, f"missing clone: {path}")
        subprocess.run(["git", "clone", *extra, url, str(path)], check=True)
    head = subprocess.run(["git", "-C", str(path), "rev-parse", "HEAD"],
                          capture_output=True, text=True, check=True).stdout.strip()
    if head != commit:
        require(not verify_only, f"{path} is at {head}, not the locked {commit}")
        subprocess.run(["git", "-C", str(path), "checkout", "--detach", commit], check=True)
        head = subprocess.run(["git", "-C", str(path), "rev-parse", "HEAD"],
                              capture_output=True, text=True, check=True).stdout.strip()
        require(head == commit, f"{path} is at {head} after checkout, not {commit}")


def _matches(path: Path, digest: str) -> bool:
    return path.is_file() and sha(path) == digest


def canary(root: Path, units: int = 2, manifest_path: Path = None) -> None:
    """Re-emit the first units of the linked module from the link manifest and compare their digests.

    The manifest records each unit's bitcode command and the digest of the module that command
    produced. Matching digests show that the toolchain and the built tree still reproduce the
    pinned bitcode; a mismatch means drift, and the restore stops.
    """
    manifest_path = manifest_path or HERE / "results" / "evaluation" / "link-manifest.json"
    manifest = json.loads(manifest_path.read_text())
    emitted = [o for o in manifest["objects"] if o.get("imported") and o.get("module_sha256")][:units]
    require(len(emitted) == units, "the link manifest has too few emitted units for the canary")
    tree = root / "php-cli" / "php-src"
    for record in emitted:
        with tempfile.TemporaryDirectory() as scratch:
            command = [resolve_record(x) for x in record["bitcode_command"]]
            output = command.index("-o") + 1
            command[output] = str(Path(scratch) / "unit.bc")
            subprocess.run(command, cwd=tree, check=True)
            actual = sha(Path(scratch) / "unit.bc")
        require(actual == record["module_sha256"],
                f"canary unit {record['object']} differs from its manifest digest: drift in the toolchain or tree")


def main(argv=None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", type=Path, default=Path.home() / ".cache/php-catalog-evaluation/root")
    parser.add_argument("--verify-only", action="store_true")
    args = parser.parse_args(argv)
    root = args.root
    root.mkdir(parents=True, exist_ok=True)
    try:
        check_disk(root)
        check_packages(root, args.verify_only)
        deps = root / "deps" / "usr"
        check_clone(root / "php-src", PHP_REPOSITORY, LOCK["php_commit"], args.verify_only,
                    ["--branch", "php-8.5.4"])
        check_clone(root / "SVF", SVF_REPOSITORY, LOCK["svf_commit"], args.verify_only, [])
        svf_digests = {"bin/dvf": LOCK["svf_binary_sha256"]["bin/dvf"],
                       "bin/wpa": LOCK["svf_binary_sha256"]["bin/wpa"],
                       "lib/extapi.bc": LOCK["svf_extapi_sha256"]}
        svf_root = root / "svf-pathfree"
        svf_output = svf_root / "svf-build"
        if not all(_matches(svf_output / name, digest) for name, digest in svf_digests.items()):
            require(not args.verify_only, "the SVF build is missing or differs; run without --verify-only")
            subprocess.run([str(HERE / "build_svf_pathfree.sh"), str(svf_root),
                            str(root / "SVF"), str(deps)], check=True)
        for name, digest in svf_digests.items():
            require(sha(svf_output / name) == digest, f"SVF output differs from the lock: {name}")
        php_cli = root / "php-cli"
        if not _matches(php_cli / "php-src/sapi/cli/php", LOCK["php_cli_sha256"]):
            require(not args.verify_only, "the php CLI is missing or differs; run without --verify-only")
            subprocess.run([str(HERE / "build_php_pathfree.sh"), str(php_cli),
                            str(root / "php-src"), str(deps)], check=True)
        require(sha(php_cli / "php-src/sapi/cli/php") == LOCK["php_cli_sha256"],
                "php CLI differs from the lock")
        call = LOCK.get("php_call_cli_sha256")
        require(call is not None, "the lock has no php_call_cli_sha256; the CALL build is not pinned")
        php_call = root / "php-call"
        if not _matches(php_call / "php-src/sapi/cli/php", call):
            require(not args.verify_only, "the CALL php CLI is missing or differs; run without --verify-only")
            subprocess.run([str(HERE / "build_php_pathfree.sh"), str(php_call),
                            str(root / "php-src"), str(deps), "CALL"], check=True)
        require(sha(php_call / "php-src/sapi/cli/php") == call, "CALL php CLI differs from the lock")
    except (RestoreError, subprocess.CalledProcessError) as error:
        print(f"restore stopped: {error}", file=sys.stderr)
        return 1
    try:
        canary(root)
    except (RestoreError, subprocess.CalledProcessError) as error:
        print(f"restore stopped: {error}", file=sys.stderr)
        return 1
    print(f"restored and verified under {root}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
