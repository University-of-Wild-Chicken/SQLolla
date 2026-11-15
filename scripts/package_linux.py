"""Assemble a private Linux x86-64 release from native, verified build outputs."""
from pathlib import Path
import json
import os
import platform
import shutil
import subprocess
import sys
import tarfile

from package_iis import ALLOY_SHA, ROOT, copy_file, create_java_runtime, dependency_notices, digest


def copy_native_release(output: Path) -> None:
    """Retain executable modes and select only required runtime files."""
    release = ROOT / "target/release"
    for name in ("sqlolla-host", "sqlolla"):
        copy_file(release / name, output / "bin" / name)
    copy_file(ROOT / "parser/.lake/build/bin/sqlolla_parser", output / "parser/sqlolla_parser")
    copy_file(ROOT / "bridge/classes/AlloyBridge.class", output / "bridge/classes/AlloyBridge.class")
    alloy = Path(os.environ["SQLolla_ALLOY_JAR"])
    if digest(alloy) != ALLOY_SHA:
        raise ValueError("Alloy artifact differs from the validated 6.2 pin")
    copy_file(alloy, output / "runtime/alloy/org.alloytools.alloy.dist.jar")
    shutil.copytree(ROOT / "frontend/dist", output / "frontend/dist")
    for name in ("start-sqlolla.sh", "README.md", "Dockerfile"):
        copy_file(ROOT / "deployment/linux" / name, output / name)
    (output / "start-sqlolla.sh").chmod(0o755)


def write_manifest(output: Path) -> None:
    """Bind a release to its actual binary bytes and the stated Linux build environment."""
    manifest_path = output / "manifest.json"
    files = [{"path": path.relative_to(output).as_posix(), "sha256": digest(path), "bytes": path.stat().st_size,
              "mode": format(path.stat().st_mode & 0o777, "04o")}
             for path in sorted(output.rglob("*")) if path.is_file() and path != manifest_path]
    manifest = {"schema": "sqlolla.linux_package.v1", "target": "linux-x86_64",
                "source_base_commit": subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=ROOT, text=True).strip(),
                "libc_build": subprocess.check_output(["getconf", "GNU_LIBC_VERSION"], text=True).strip(),
                "parser_commit": "ad15c585155ff7fc2c5fc4c9746905e4959ff5eb", "files": files,
                "distribution": "Private deployment; SQLeanParser redistribution rights unresolved."}
    manifest_path.write_text(json.dumps(manifest, indent=2) + "\n", encoding="utf-8")
    manifest_path.chmod(0o644)


def release_modes(output: Path) -> None:
    """Allow deployment under a separate unprivileged account without writable shared code."""
    for path in (output, *output.rglob("*")):
        if not path.resolve().is_relative_to(output):
            raise ValueError("Release files must resolve inside staging")
        if not path.is_symlink():
            location = path.relative_to(output).parts
            java_executable = location[:2] == ("runtime", "java") and path.stat().st_mode & 0o111
            executable = path.is_dir() or path.suffix == ".sh" or location[:1] in (("bin",), ("parser",)) or java_executable
            path.chmod(0o755 if executable else 0o644)


def archive(output: Path) -> Path:
    """Emit a checked tarball and checksum without including mutable instance state."""
    manifest = json.loads((output / "manifest.json").read_text(encoding="utf-8"))
    target = output.with_suffix(".tar.gz")
    temporary = target.with_suffix(".gz.partial")
    with tarfile.open(temporary, "w:gz", compresslevel=6) as package:
        for entry in manifest["files"]:
            path = (output / entry["path"]).resolve()
            if not path.is_relative_to(output) or digest(path) != entry["sha256"]:
                raise ValueError("Staged package does not match its inventory")
            package.add(path, arcname=f"{output.name}/{entry['path']}", recursive=False)
        package.add(output / "manifest.json", arcname=f"{output.name}/manifest.json")
    temporary.replace(target)
    target.with_suffix(".gz.sha256").write_text(f"{digest(target)}  {target.name}\n", encoding="ascii")
    return target


def main() -> None:
    """Require a new release directory under the ignored publication folder."""
    if sys.platform != "linux" or platform.machine() != "x86_64" or len(sys.argv) != 2:
        raise SystemExit("Run on Linux x86-64: python3 scripts/package_linux.py <new staging directory>")
    output = Path(sys.argv[1]).resolve()
    publication = (ROOT / "target/publication").resolve()
    if output == publication or not output.is_relative_to(publication):
        raise ValueError("Staging must be a fresh child of target/publication")
    output.mkdir(parents=True, exist_ok=False)
    copy_native_release(output)
    create_java_runtime(output)
    dependency_notices(output, Path(os.environ["LEAN_BIN"]).resolve().parent)
    release_modes(output)
    write_manifest(output)
    print(archive(output))


if __name__ == "__main__":
    main()
