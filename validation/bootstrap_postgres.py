"""Fetch the pinned EDB PostgreSQL16 Windows archive without an installer or service."""
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
from urllib.request import urlopen
import zipfile

from pg_runtime import ROOT
from process_helpers import process_options

VERSION = "16.16"
ARCHIVE_NAME = "postgresql-16.16-1-windows-x64-binaries.zip"
URL = "https://get.enterprisedb.com/postgresql/" + ARCHIVE_NAME
SHA256 = "5c0d06f3d26a929ecb36dd931409f1e51a4181355794a259b90aae8bffc57a9f"
CACHE = ROOT / "tools/.tools/postgres-download"
DESTINATION = ROOT / "tools/.tools/postgres-16.16"


def archive_hash(path: Path) -> str:
    """Hash the archive as bytes without loading its full contents into memory."""
    checksum = hashlib.sha256()
    with path.open("rb") as source:
        for block in iter(lambda: source.read(1024 * 1024), b""):
            checksum.update(block)
    return checksum.hexdigest()


def verified_archive() -> Path:
    """Reuse or download official bytes and require the reviewed publisher SHA256."""
    CACHE.mkdir(parents=True, exist_ok=True)
    archive = CACHE / ARCHIVE_NAME
    if not archive.exists():
        with urlopen(URL, timeout=60) as source, archive.open("wb") as target:
            shutil.copyfileobj(source, target)
    if archive_hash(archive) != SHA256:
        raise ValueError("PostgreSQL archive checksum mismatch; inspect the cached file")
    return archive


def extract_runtime(archive: Path) -> int:
    """Extract only bin/lib/share after checking each resolved path stays inside tools."""
    root = DESTINATION.resolve()
    count = 0
    with zipfile.ZipFile(archive) as package:
        for entry in package.infolist():
            if entry.is_dir() or not entry.filename.startswith(("pgsql/bin/", "pgsql/lib/", "pgsql/share/")): continue
            target = (root / entry.filename).resolve()
            if not target.is_relative_to(root): raise ValueError("Archive path escapes destination")
            target.parent.mkdir(parents=True, exist_ok=True)
            with package.open(entry) as source, target.open("wb") as output:
                shutil.copyfileobj(source, output)
            count += 1
    return count


def main() -> None:
    """Install only an isolated verified runtime and verify its reported major/patch."""
    if os.name != "nt": raise SystemExit("Use an installed PostgreSQL16 and set SQLolla_PG_BIN on this platform")
    archive = verified_archive()
    executable = DESTINATION / "pgsql/bin/postgres.exe"
    count = 0 if executable.exists() else extract_runtime(archive)
    actual = subprocess.check_output([str(executable), "--version"], encoding="utf-8", **process_options()).strip()
    if actual != "postgres (PostgreSQL) " + VERSION: raise ValueError("Unexpected PostgreSQL version")
    print(json.dumps({"version": actual, "archive_sha256": SHA256, "files_extracted": count, "global_install": False}))


if __name__ == "__main__":
    main()
