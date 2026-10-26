# Rust CI tooling

`bonsai-ci` replaces the repository's Python impact planner, regression tests and
aggregate gate. It reads Cargo manifests directly and uses the existing routing
policy in `.github/ci/impact.json`. It never builds inspected packages or runs
Cargo metadata. Git is required for ancestor and diff queries; HTTPS baseline
lookup uses Rust with a 20-second timeout and the supplied `GITHUB_TOKEN`.

```powershell
$scratch = Join-Path ([IO.Path]::GetTempPath()) ('Bonsai-CI-' + [guid]::NewGuid().ToString('N'))
cargo build --locked --package bonsai-ci --target-dir $scratch
& (Join-Path $scratch 'debug/bonsai-ci.exe') impact --root $PWD --full
```

`impact` uses `GITHUB_WORKSPACE`, falling back to the current directory, unless
`--root` is supplied. `--config` overrides the policy path. Manual events and
`--full` select all groups. Push events compare `--after` (default `HEAD`) against
the newest successful main-push ancestor for `--workflow`. Missing credentials,
API/diff failure or an unusable baseline select full coverage, never the event's
unvalidated parent. Without `--workflow`, `--before` supplies the baseline.
Invalid UTF-8 diff paths also select full coverage.

The compact JSON plan is printed to stdout. If provided, `GITHUB_OUTPUT` receives
the plan, package list, boolean groups and filtered matrices;
`GITHUB_STEP_SUMMARY` receives a readable selection summary.

`gate --workflow foundation` reads `CI_NEEDS`, verifies the exact configured job
set and rejects planner failure, malformed results and unsuccessful selected
jobs. Only deliberately unselected skips are permitted.

The routing, baseline, output and gate regressions are Rust tests. Use named
focused selections locally with external targets/working directories; complete
suites remain CI-only under [the validation policy](../../docs/validation.md).
