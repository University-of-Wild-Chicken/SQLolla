"""Acquire commit-pinned authoritative DDL without running downloaded application code."""
import hashlib
import json
from pathlib import Path
import urllib.request

from run import ROOT, digest, write_json

REPOSITORIES = {
    "benchbase": ("cmu-db/benchbase", "33c00473807ebd49304d114a6d769d2d2b2bbb34"),
    "river": ("riverqueue/river", "1354a059d7cbe5790acc7a755a2ab7ac6d2f1ed8"),
    "temporal": ("temporalio/temporal", "b7bc978d1040afe2dcbc68ecfe8b228ae198e0b6"),
}
MANIFEST = ROOT / "validation/corpus-manifest.json"


def download(url: str) -> bytes:
    """Fetch only an explicitly constructed HTTPS primary-source URL."""
    request = urllib.request.Request(url, headers={"User-Agent": "SQLolla-validation"})
    with urllib.request.urlopen(request, timeout=30) as response:
        return response.read()


def selected(population: str, path: str) -> bool:
    """Select PostgreSQL DDL for exactly the benchmark and application populations."""
    if population == "benchbase":
        ddl = {f"src/main/resources/benchmarks/{name}/ddl-postgres.sql" for name in ["smallbank", "tatp", "tpcc", "tpch", "twitter", "ycsb"]}
        drivers = {f"src/main/java/com/oltpbenchmark/benchmarks/tpch/procedures/{name}.java" for name in ["Q1", "Q15"]}
        return path in ddl | drivers
    if population == "river":
        return path.startswith("riverdriver/riverpgxv5/migration/main/") and path.endswith(".up.sql")
    return path == "schema/postgresql/v12/temporal/schema.sql"


def acquire(population: str, repository: str, commit: str) -> list[dict]:
    """Verify raw bytes against the pinned Git tree blob ID and retain SHA256."""
    tree_url = f"https://api.github.com/repos/{repository}/git/trees/{commit}?recursive=1"
    tree = json.loads(download(tree_url))
    if tree.get("truncated"): raise ValueError("Refusing incomplete upstream tree")
    sources = []
    for entry in tree["tree"]:
        if entry["type"] != "blob" or not selected(population, entry["path"]): continue
        path = entry["path"]
        url = f"https://raw.githubusercontent.com/{repository}/{commit}/{path}"
        data = download(url)
        blob = hashlib.sha1(b"blob " + str(len(data)).encode() + b"\0" + data).hexdigest()
        if blob != entry["sha"]: raise ValueError("Upstream raw file does not match pinned Git blob")
        target = ROOT / "validation/upstream" / population / path
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_bytes(data)
        sources.append({"population": population, "path": str(target.relative_to(ROOT)).replace("\\", "/"),
                        "upstream_path": path, "source_url": url, "source_commit": commit,
                        "git_blob_sha1": blob, "sha256": digest(data), "bytes": len(data),
                        "status": "downloaded_hash_verified_not_executed",
                        "kind": "authoritative_upstream_ddl" if path.endswith(".sql") else "authoritative_driver_context"})
    return sources


def main() -> None:
    """Attach primary DDL provenance to the single maintained corpus ledger."""
    manifest = json.loads(MANIFEST.read_text(encoding="utf-8"))
    manifest["context_sources"] = [source for name, (repository, commit) in REPOSITORIES.items() for source in acquire(name, repository, commit)]
    write_json(MANIFEST, manifest)
    print(json.dumps({"downloaded_verified_ddl_files": len(manifest["context_sources"])}))


if __name__ == "__main__":
    main()
