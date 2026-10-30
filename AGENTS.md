# Outstanding work

Use [PLAN.md](PLAN.md) as the source of truth for all outstanding project work.

# Owner-device execution budgets

Diagnostics, model qualification trials and benchmarks on the owner's laptop
must fit within five minutes total, preferably less. This includes self-hosted
CI on that laptop and setup, loading, comparison and cleanup. Shrink the trial
or leave the gate pending when it cannot fit; do not divide a long matrix into
repeated runs to work around the limit. Complete suites and benchmarks remain
CI-only; hosted cloud CI has its own workflow limits.

# Parallel work and commits

Each worker in a parallel fan-out must use its own individual Git worktree.
Within each worktree, place each logical change in its own individual commit.

# UI architecture

The first-party UI must remain native and written entirely in Rust. Use native
Rust UI libraries or Rust bindings to operating-system controls. Do not add a
JavaScript/TypeScript frontend, HTML/CSS UI, browser/WebView shell, Electron, or
a web-rendered Tauri frontend. PowerShell may provision, build and launch the
Rust application.

Keep interoperability separate from presentation. Preserve documented local
LLM HTTP/JSON/SSE conventions and explicit supported-subset boundaries rather
than requiring a web UI or claiming complete OpenAI API compatibility.

Read [the UI architecture contract](docs/ui-architecture.md) before changing
the UI, its dependencies, transport or launcher.
