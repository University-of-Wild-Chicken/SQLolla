# SQLolla working contract

Rust owns binding, typed RA, interpretation, Alloy generation and orchestration.
Lean supplies SQLeanParser plus formal specifications; Java17 only bridges Alloy.
Never introduce Lean proof placeholders, custom/unapproved axioms, unsafe proof
shortcuts or native_decide. Run formal/verify.ps1 and the parser trust gate after
changing Lean. Standard foundations are explicitly listed in the trust gate.

Authored functions/methods must have at most 30 physical lines after formatting,
with purpose documentation and explicit effects. Preserve SQL bags, NULL/3VL,
typed parameter identity, output positions and deterministic compilation.
Unsupported syntax or missing semantic coverage must fail closed.

M1 is an offline relational compiler/analyzer. Do not imply transaction,
concurrency, PostgreSQL-wide, hosted-service or AI correctness from its tests.
Use real PostgreSQL16 differential tests and independent Alloy-result checks.

Begin each milestone with a kernel-checked Lean specification. Preserve M0/M1
invariants and run the pinned industrial SQL census plus applicable actual
PostgreSQL industrial replays on each semantic increment. Keep syntax/export
coverage separate from execution coverage. M2 implementation is now authorized;
maintained contracts are in contracts/m2. AI feasibility artifacts stay in .scratch/m3.

Use explicitly requested GPT-6.1 Sol subagents as the AI feasibility baseline.
Production AI flows pin the model to the user's selected BYOT provider.
OpenAI uses gpt-6.1-sol, Anthropic uses claude-sonnet-5-5, and Gemini uses
gemini-4-hydrogen. Verify live availability; never silently substitute another
model or provider. AI outputs are untrusted proposals
or explanations and must never replace deterministic binding, solver verdicts,
policy checks or replay. A small subagent study is not a provider benchmark.

After each pass, remove task-owned temporary experiments, generated duplicate
reports and obsolete scaffolding. Retain focused source/tests, pinned dependency
provenance, essential reproducible evidence and maintained architecture/run docs.
Verify resolved deletion targets; never delete user work or active toolchains.
