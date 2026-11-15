# Historical M0 evidence index

This directory records the initial feasibility stage. Statements below describe
that stage, before the implementation and PostgreSQL/browser/provider validation
now documented in the [current project README](../README.md).

The architecture decisions are in [architecture.json](architecture.json) and the
human-readable [architecture](../ARCHITECTURE.md). Each probe is disposable
feasibility work; no production backend, frontend or deployment has been built.

| Evidence | What it establishes |
| --- | --- |
| [formal/report.json](formal/report.json) | Lean build, source inventory and compiled trust gate |
| [parser/report.json](parser/report.json) | Native parser/JSON specimen and finite repeat tests |
| [alloy/report.json](alloy/report.json) | Real Java17 Alloy API, SAT4J, SAT/UNSAT and numeric probes |
| [portability/report.json](portability/report.json) | Windows Lean/Java/Rust host checks and untested platform matrix |
| [alloy/formal-review.json](alloy/formal-review.json) | Independent semantic-contract review and resolved findings |
| [instance-review.json](instance-review.json) | Pinned AlloyStudio reference review for instance presentation |

Reports include exact commands, versions, checksums, observations and limitations.
Tools/archives under `.tools`, compiled outputs and temporary native libraries
are ignored local artifacts. Retain them for reruns; do not ship them by accident.
SQLeanParser tracked sources were unchanged; its `.lake` directory was built.

Windows process/UTF-8 tests do not validate Linux/macOS packages. No PostgreSQL
instance was run, and parser acceptance is not database semantic compatibility.
The public domain and Cloudflare account were not modified.
