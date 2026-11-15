# SQLolla on Ubuntu and WSL

The private Linux x86-64 archive includes the compiled Rust host and CLI, the native SQLeanParser adapter, Java 17 and Alloy 6.2, and the bundled web UI. Build and deploy on a compatible glibc distribution. The supported reference target is Ubuntu 26.04 x86-64, including WSL2. The package lists its compiler inputs and per-file hashes in `manifest.json`. Check the archive against the accompanying SHA256 file before extracting it.

```sh
sha256sum -c sqlolla-linux-x64.tar.gz.sha256
tar -xzf sqlolla-linux-x64.tar.gz
SQLolla_STATE="$HOME/.local/state/sqlolla" ./sqlolla-linux-x64/start-sqlolla.sh
```

Open `http://127.0.0.1:8787`. The default uses a private, persistent instance directory. Set `SQLolla_HOST_PORT` if this port is occupied. Run as an unprivileged account. For a reverse proxy, set `SQLolla_PUBLIC_ORIGIN=https://sqlolla.us` and forward the original `Host`, `Origin`, `X-SQLolla-CSRF` and `Authorization` headers. Keep HTTPS and the existing Cloudflare Access policy on the same origin. Handle API responses without HTML error substitution or caching.

The runtime does not need a database, Lean, Rust or Node. PostgreSQL 16 is used to validate the compiler during development. SQL runs in the bounded analysis model. OpenAI explanations and regeneration use `gpt-6.1-sol` and a key kept in the browser tab, sent only with the user's explicit request.

## Container option

From the extracted archive, build the bundled Dockerfile on Linux x86-64. Run with Linux host networking so the application's loopback listener stays on the host's loopback. Use a named volume for the instance store and point your HTTPS proxy at that listener.

```sh
docker build -t sqlolla:private ./sqlolla-linux-x64
docker volume create sqlolla-data
docker run --rm --name sqlolla --network host \
  -e SQLolla_PUBLIC_ORIGIN=https://sqlolla.us \
  -v sqlolla-data:/var/lib/sqlolla sqlolla:private
```

For local HTTP use omit `SQLolla_PUBLIC_ORIGIN`. Windows Docker Desktop uses a VM and has its own host-networking behavior. The verified reference is native Linux/WSL, with the reverse proxy placed on the same host. Use the non-container launcher if the container cannot reach the intended loopback interface.

The image uses `SIGINT` for `docker stop`, which starts the host's graceful shutdown. Let active analyses finish before stopping the container for migration.

## Move a private IIS deployment

Pause traffic and back up the Windows instance store and the browser's review journal separately. Keep the prior IIS release with the store. On Linux, launch with its own writable instance directory and run a sample query through the HTTPS proxy. Confirm the SQL, IR and Alloy model appear, test Next, then connect OpenAI with a fresh key and explicitly request an explanation. Move the existing Cloudflare origin only after these checks pass. Preserve the public origin to keep browser-local review history.

Instance archives bind their exact compiler bytes and may refuse replay after an OS or binary change. Keep original archived evidence and its matching executable available for review, and reanalyze source queries to produce Linux-native cases. Retain old labels for reference and withdraw those without compatible archive evidence before regeneration. Restore the prior proxy target and unchanged Windows store to roll back.

## Rebuild

Use Rust 1.100.0, Lean 4.34.0, Temurin 17.0.21+9 and Node 24.22.1. Place SQLeanParser at the pinned commit in `../dependencies/SQLeanParser`. The maintained census currently expects Windows checkout bytes, so set `core.autocrlf=true` in that dependency before checkout. Set `LEAN_BIN`, `SQLolla_JAVA`, `SQLolla_JAVAC` and `SQLolla_ALLOY_JAR` to the installed toolchain paths. The build verifies the Alloy JAR digest.

```sh
sh scripts/build.sh
cargo test --workspace --locked
cargo fetch --locked
cargo build --release --workspace --locked
(cd frontend && npm ci && npm run check)
python3 scripts/package_linux.py target/publication/sqlolla-linux-x64
```

The package includes dependency notices. The SQLeanParser repository's root redistribution terms are unresolved. Use the archive for the private deployment and confirm those terms before public redistribution.
