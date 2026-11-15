"""Assemble an offline private Windows x64 IIS release from verified local build outputs."""
from __future__ import annotations

import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import zipfile

ROOT = Path(__file__).resolve().parents[1]
ALLOY_SHA = "6b8c1cb5bc93bedfc7c61435c4e1ab6e688a242dc702a394628d9a9801edb78d"


def digest(path: Path) -> str:
    """Hash artifact bytes without loading large runtimes entirely into memory."""
    with path.open("rb") as stream:
        return hashlib.file_digest(stream, "sha256").hexdigest()


def copy_file(source: Path, output: Path) -> None:
    """Copy one explicitly selected artifact while preserving dependency bytes."""
    output.parent.mkdir(parents=True, exist_ok=True)
    shutil.copy2(source, output)


def copy_artifacts(output: Path) -> None:
    """Select release executables and web assets; exclude stores, keys, source and development caches."""
    release = ROOT / "target/iis-win-x64/release"
    for name in ("sqlolla-host.exe", "sqlolla.exe"):
        copy_file(release / name, output / "bin" / name)
    copy_file(ROOT / "parser/.lake/build/bin/sqlolla_parser.exe", output / "parser/sqlolla_parser.exe")
    copy_file(ROOT / "bridge/classes/AlloyBridge.class", output / "bridge/classes/AlloyBridge.class")
    alloy = Path(os.environ["SQLolla_ALLOY_JAR"])
    if digest(alloy) != ALLOY_SHA:
        raise ValueError("Alloy artifact differs from the validated pin")
    copy_file(alloy, output / "runtime/alloy/org.alloytools.alloy.dist.jar")
    shutil.copytree(ROOT / "frontend/dist", output / "frontend/dist")
    for name in ("Start-SQLolla.ps1", "Test-Package.ps1", "deployment.json", "README.md"):
        copy_file(ROOT / "deployment/iis" / name, output / name)
    copy_file(ROOT / "deployment/iis/web.config", output / "iis/wwwroot/web.config")


def create_java_runtime(output: Path) -> None:
    """Use the pinned Java17 jlink to build an offline runtime including standard Java APIs."""
    java_home = Path(os.environ["SQLolla_JAVA"]).resolve().parents[1]
    jlink = java_home / "bin" / ("jlink.exe" if os.name == "nt" else "jlink")
    subprocess.run([str(jlink), "--add-modules", "java.se,jdk.unsupported,jdk.crypto.ec",
                    "--strip-debug", "--no-header-files", "--no-man-pages", "--compress=2", "--output",
                    str(output / "runtime/java")], check=True,
                   creationflags=subprocess.CREATE_NO_WINDOW if os.name == "nt" else 0)
    copy_file(java_home / "NOTICE", output / "runtime/java/NOTICE")


def dependency_notices(output: Path, lean: Path | None = None) -> None:
    """Retain pinned license texts and explicit unresolved parser distribution provenance."""
    copy_file(ROOT / "LICENSE", output / "notices/SQLolla-LICENSE")
    lean = lean or ROOT / "m0/parser/.tools/lean-4.34.0-windows"
    for source in lean.glob("LICENSE*"):
        copy_file(source, output / "notices/Lean" / source.name)
    metadata = subprocess.check_output(["cargo", "metadata", "--locked", "--offline", "--format-version=1"], cwd=ROOT)
    packages = json.loads(metadata)["packages"]
    inventory = [{"name": p["name"], "version": p["version"], "license": p["license"], "source": p["source"]} for p in packages]
    for package in packages:
        directory = Path(package["manifest_path"]).parent
        for source in directory.iterdir():
            if source.is_file() and source.name.upper().startswith(("LICENSE", "COPYING", "NOTICE")):
                copy_file(source, output / "notices/rust" / f'{package["name"]}-{package["version"]}' / source.name)
    (output / "notices/rust-packages.json").write_text(json.dumps(inventory, indent=2), encoding="utf-8")
    node_notices(output)


def node_notices(output: Path) -> None:
    """Preserve runtime frontend package license files, including local fonts and Graphviz."""
    lock = json.loads((ROOT / "frontend/package-lock.json").read_text(encoding="utf-8"))
    for relative, entry in lock["packages"].items():
        if not relative or entry.get("dev"):
            continue
        package = ROOT / "frontend" / relative
        for source in package.rglob("*"):
            if source.is_file() and source.name.upper().startswith(("LICENSE", "COPYING", "NOTICE", "OFL")):
                copy_file(source, output / "notices/frontend" / relative / source.relative_to(package))
    with zipfile.ZipFile(Path(os.environ["SQLolla_ALLOY_JAR"])) as jar:
        for name in jar.namelist():
            if not name.endswith("/") and any(word in Path(name).name.upper() for word in ("LICENSE", "NOTICE")):
                target = output / "notices/alloy" / name
                if not target.resolve().is_relative_to((output / "notices/alloy").resolve()):
                    raise ValueError("Unsafe JAR notice path")
                target.parent.mkdir(parents=True, exist_ok=True)
                target.write_bytes(jar.read(name))


def write_manifest(output: Path) -> None:
    """Bind immutable payload hashes to source revision and the exact private deployment scope."""
    excluded = {"manifest.json", "deployment.json", "iis/wwwroot/web.config"}
    files = [{"path": p.relative_to(output).as_posix(), "sha256": digest(p), "bytes": p.stat().st_size}
             for p in payload_files(output) if p.relative_to(output).as_posix() not in excluded]
    revision = subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=ROOT, text=True).strip()
    dirty = bool(subprocess.check_output(["git", "status", "--porcelain"], cwd=ROOT).strip())
    manifest = {"schema": "sqlolla.iis_package.v1", "target": "Windows 10 IoT Enterprise LTSC 2021 (21H2), x64",
                "source_base_commit": revision, "working_tree_changes_included": dirty,
                "parser_commit": "ad15c585155ff7fc2c5fc4c9746905e4959ff5eb", "files": files,
                "mutable_configuration": ["deployment.json", "iis/wwwroot/web.config"], "public_origin": "https://sqlolla.us",
                "deployment_scope": "private trusted-user appliance; no per-user server isolation",
                "distribution_note": "SQLeanParser root redistribution rights unresolved; private user deployment only; review before redistribution"}
    (output / "manifest.json").write_text(json.dumps(manifest, indent=2) + "\n", encoding="utf-8")


def payload_files(output: Path) -> list[Path]:
    """Exclude obsolete hashed frontend assets when refreshing an existing task-owned stage."""
    result = []
    for path in sorted(output.rglob("*")):
        relative = path.relative_to(output)
        if not path.is_file():
            continue
        if relative.as_posix().startswith("frontend/dist/") and not (ROOT / relative).is_file():
            continue
        result.append(path)
    return result


def archive(output: Path) -> Path:
    """Publish only the explicit manifest inventory and configuration, never stray staging artifacts."""
    manifest = json.loads((output / "manifest.json").read_text(encoding="utf-8"))
    expected = {entry["path"]: entry["sha256"] for entry in manifest["files"]}
    paths = [entry["path"] for entry in manifest["files"]] + manifest["mutable_configuration"] + ["manifest.json"]
    target = output.with_suffix(".zip")
    temporary = target.with_suffix(".zip.partial")
    with zipfile.ZipFile(temporary, "w", compression=zipfile.ZIP_DEFLATED, compresslevel=6, strict_timestamps=False) as bundle:
        for name in sorted(paths):
            source = (output / name).resolve()
            if not source.is_relative_to(output.resolve()):
                raise ValueError("Archive path escaped staging root")
            if name in expected and digest(source) != expected[name]:
                raise ValueError(f"Payload changed after verification: {name}")
            bundle.write(source, output.name + "/" + name)
    temporary.replace(target)
    target.with_suffix(".zip.sha256").write_text(digest(target) + "  " + target.name + "\n", encoding="ascii")
    return target


def main() -> None:
    """Create a fresh release directory; never overwrite another build or user data."""
    if os.name != "nt" or len(sys.argv) not in (2, 3):
        raise SystemExit("Run on Windows: python scripts/package_iis.py [--archive] <staging directory>")
    publish = len(sys.argv) == 3 and sys.argv[1] == "--archive"
    if len(sys.argv) == 3 and not publish:
        raise SystemExit("Unknown option")
    output = Path(sys.argv[-1]).resolve()
    publication = (ROOT / "target/publication").resolve()
    if output == publication or not output.is_relative_to(publication):
        raise ValueError("Staging directory must be a fresh child of target/publication")
    if publish:
        print(json.dumps({"zip": str(archive(output)), "status": "packaged"}))
        return
    output.mkdir(parents=True, exist_ok=False)
    copy_artifacts(output)
    create_java_runtime(output)
    dependency_notices(output)
    write_manifest(output)
    print(json.dumps({"stage": str(output), "status": "assembled_not_yet_tested"}))


if __name__ == "__main__":
    main()
