# SQLolla private IIS publication

This ZIP targets **Windows 10 IoT Enterprise LTSC 2021, version 21H2, build 19044, x64**. It is a private, trusted-user appliance: the server has one shared evidence store and no per-user authorization. Put the entire site behind a VPN, a restricted LAN, or an authenticated perimeter such as Cloudflare Access with origin access restricted. A domain name and CSRF protection do not provide user authentication. Do not expose an unauthenticated shared evidence store to the Internet.

The package includes a static-CRT Rust release, the native SQLeanParser adapter, Java 17 runtime, Alloy 6.2 JAR, compiled bridge and local frontend/Graphviz/fonts. It does not require Rust, Lean, Node, npm, PostgreSQL, a JDK or build tools on the server. PostgreSQL is used in development differential tests; SQLolla never connects to a production database. BYOT credentials for OpenAI, Anthropic, or Gemini enter only through the browser's memory-only settings and explicit requests.

Only `iis/wwwroot` is the IIS site's physical directory. All requests are reverse-proxied to the Rust host, which serves the private `frontend/dist` assets. **Never point IIS at the extracted package root or the evidence directory.** No API key, certificate private key, user journal or instance archive is included. `manifest.json` lists immutable payload hashes; `deployment.json` and `iis/wwwroot/web.config` are intentionally editable configuration.

## Prepare the server

1. Apply the current servicing updates for the exact IoT LTSC SKU. Confirm 64-bit Windows and free RAM for the bounded solver (Java heap limit 512 MiB, plus Rust/frontend/request data). Microsoft's [release history](https://learn.microsoft.com/en-us/windows/iot/iot-enterprise/whats-new/release-history) identifies 19044 and the IoT LTSC 2021 servicing period. Windows desktop IIS has a [concurrent-request restriction](https://learn.microsoft.com/en-us/iis/troubleshoot/request-restrictions); this is a home demo deployment, not a throughput claim.
2. Enable IIS Management Console and World Wide Web Services. Install Microsoft's x64 [URL Rewrite](https://www.iis.net/downloads/microsoft/url-rewrite) and [Application Request Routing](https://www.iis.net/downloads/microsoft/application-request-routing). The [Microsoft reverse-proxy guide](https://learn.microsoft.com/en-us/iis/extensions/url-rewrite-module/reverse-proxy-with-url-rewrite-v2-and-application-request-routing) describes the required modules. These machine-wide prerequisites are not installed by this package.
3. Extract to a new version directory, for example `C:\Program Files\SQLolla\releases\2026-11-15`. Run `Test-Package.ps1` in Windows PowerShell 5.1 or PowerShell 7. The SHA256 sidecar verifies the ZIP; the manifest detects accidental changes, but neither is a publisher signature. Scripts are unsigned. After checking provenance, unblock the downloaded scripts if Windows marks them as remote.
4. Create a dedicated nonadministrator local account, for example `sqlolla_runner`. Grant it read/execute on this release and Modify on `C:\ProgramData\SQLolla`. Grant administrators/SYSTEM full control. Remove inherited broad-user access from the data folder. Give the IIS application-pool identity read access only to `iis/wwwroot`; it does not need the private binaries or evidence. Create data/temp folders before startup so permissions are deliberate. Do not grant the runner write access to the release binaries.
5. Review `deployment.json`: `public_origin` defaults to `https://sqlolla.us`, port to `8787`, and private data to `C:\ProgramData\SQLolla`. Origin must be canonical HTTPS with no path, trailing slash or explicit `:443`. For another host/port, change the exact Host regex and loopback port in `web.config` too. Keep evidence outside release/site folders. On systems using [Unified Write Filter](https://learn.microsoft.com/en-us/windows/configuration/unified-write-filter/uwf-volumeaddexclusion), place private data on an approved persistent volume/exclusion.

## Configure IIS and HTTPS

Create a dedicated site and application pool (No Managed Code), using `<release>\iis\wwwroot` as its physical directory. Add an HTTPS binding for `sqlolla.us` with the appropriate certificate; do not add an anonymous HTTP publishing path. Enable ARR proxy mode, **preserve the original Host header**, and set its proxy timeout to 360 seconds. These are server-level ARR settings; review the effect on other sites before applying them. For a dedicated IIS installation, an elevated shell can use:

```powershell
$appcmd = "$env:windir\System32\inetsrv\appcmd.exe"
& $appcmd set config /section:system.webServer/proxy /enabled:true /preserveHostHeader:true /timeout:"00:06:00" /commit:apphost
```

Forward Host, Origin, `X-SQLolla-CSRF`, `X-SQLolla-Provider`, content type, body and Authorization unchanged. The host ignores Forwarded/X-Forwarded headers and always listens on **127.0.0.1**. Do not add CORS, rewrite Origin or remove the CSRF check. BYOT uses Authorization for the provider credential: do not configure an IIS authentication mechanism that consumes/replaces that header. Keep perimeter authentication separate. Disable ARR response caching and custom error replacement for this site. Disable Failed Request Tracing and any header/body logging that could capture Authorization or SQL. Ordinary access logs should contain only standard request metadata.

For Cloudflare, configure the DNS record yourself, use end-to-end TLS with **Full (strict)**, bypass caching for this application, and apply the chosen access policy to every route including `/api/*`. Restrict direct origin access to the trusted ingress. No DNS, firewall, certificate or Cloudflare settings are changed by this package. Synchronous checks can outlast an upstream proxy's request timeout: a timeout is a transport failure, not a solver verdict. Long-running deployments may require an asynchronous jobs interface in a later release.

## Start and accept the release

Run `powershell.exe -NoProfile -ExecutionPolicy RemoteSigned -File .\Start-SQLolla.ps1` as the dedicated account for the first check, after verifying and unblocking the package. The same process-scoped policy can run `Test-Package.ps1`; organizational Group Policy still takes precedence. The launcher resolves all executable paths relative to the extracted package, keeps secrets out of configuration and writes only to the private data directory. For unattended operation use Task Scheduler: trigger **At startup**, run as the dedicated account whether logged on or not, use limited privileges, select **Do not start a new instance**, enable restart on failure, and disable the default task execution time limit. Action:

```text
Program: C:\Windows\System32\WindowsPowerShell\v1.0\powershell.exe
Arguments: -NoProfile -NonInteractive -ExecutionPolicy RemoteSigned -File "C:\Program Files\SQLolla\releases\2026-11-15\Start-SQLolla.ps1"
Start in: C:\Program Files\SQLolla\releases\2026-11-15
```

Use your actual release directory. Configure account credentials in Task Scheduler, never in a script or JSON file. The executable is a console program, **not an SCM service**; `sc.exe create` alone will not install it correctly. Ctrl-C provides graceful shutdown. Task termination/power loss is abrupt: after stopping a task, verify its own Rust/parser/Java descendants stopped before replacing binaries. Do not terminate unrelated Java processes.

Before opening access, verify on the target machine:

- `Test-Package.ps1` passes under the runner account, and the host starts after reboot without an interactive login. Check that TCP 8787 listens only on 127.0.0.1.
- Through the actual HTTPS IIS URL, load the workbench, `/docs/`, and `GET /api/status`. In the blank workbench, declare a schema and analyze a query. Confirm Graphviz and Next on a case with multiple witnesses. The regression request lives in `frontend/e2e/tenant-request.json` and can be pasted into the advanced request editor. Confirm IIS serves only the intended public routes.
- Save a case to Unsure: regeneration stays disabled. Move it to No or Yes, reopen it from its bin, relabel/withdraw, reload and confirm persistence. Keep a persistent browser profile; kiosk/InPrivate sessions erase local review history. Export the review journal before switching origin, clearing browser storage or changing machines.
- For the tenant sample, mark an unwanted tenant case No, enter the original prompt, check `SELECT id, tenant_id FROM orders WHERE tenant_id = 1`, copy it, and adopt it only after the deterministic check succeeds. Prompt or label edits must invalidate adoption. A No-only check does not preserve any unlabelled business use case.
- Cross-origin POST, missing CSRF and wrong Host fail. Confirm original 4xx/5xx JSON bodies pass through IIS. The generic status endpoint does not attest solver readiness; the actual analysis above does.
- If AI is desired, enter a short-lived BYOT key. Use Build with AI to propose a new model, accept it into the editor, and analyze it explicitly. Test case explanation and feedback-driven refinement separately. Confirm the provider consent and disconnect controls and ensure the key stays out of storage and traces.

These are target acceptance gates. Local package tests on Windows 11 do not certify IIS modules, Windows 10 optional-component configurations, Cloudflare timeout limits, account ACLs, or remote startup behavior. The parser uses the OS ICU library, documented for modern Windows in [Microsoft's ICU guidance](https://learn.microsoft.com/en-us/windows/win32/intl/international-components-for-unicode--icu-).

## Update, rollback and maintenance

Keep the prior release intact. Take the site out of traffic, allow active checks to finish, stop its task, verify descendants stopped, and back up private data plus separately exported browser review journals. Extract and test the new release into another directory, adjust the task action/site root, start and run the acceptance checks. Roll back by selecting the prior immutable release and its compatible private data backup. Archives bind the exact compiler binary hash; a different build can refuse old archives even when SQL is unchanged. Do not edit archive hashes to bypass this: retain the old binary for review/export or reanalyze explicitly. Never delete the persistent store during an upgrade.

Maintain package source under `deployment/iis`, with assembly in `scripts/package_iis.py`; update one `verification.json` rather than adding per-pass reports. The build command uses a separate release target directory and static MSVC CRT:

```powershell
. .\scripts\env.ps1
$env:CARGO_TARGET_DIR = Join-Path $PWD 'target/iis-win-x64'
$env:RUSTFLAGS = '-C target-feature=+crt-static'
cargo build --release --workspace --locked
python scripts/package_iis.py target/publication/sqlolla-iis-win-x64
```

Build and verify the frontend and Lean contracts before assembly; compile the Java bridge with `--release 17`. Run the package/runtime/browser checks, then `python scripts/package_iis.py --archive target/publication/sqlolla-iis-win-x64` creates the ZIP and SHA256 sidecar from the explicit inventory. Dependency licenses and notices are retained under `notices` and Java `legal`. **SQLeanParser's upstream root redistribution license remains unresolved** at the pinned commit; this is the user's private deployment package. Confirm rights before publicly redistributing the parser-containing package. This note does not modify third-party terms.
