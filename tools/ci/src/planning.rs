//! Successful-ancestor baselines and conservative Git diff handling.

use std::path::Path;
use std::process::Command;
use std::time::Duration;

use serde_json::Value;

use crate::Result;

pub(super) struct Inputs {
    pub(super) paths: Vec<String>,
    pub(super) full: bool,
    pub(super) reason: &'static str,
}

pub(super) struct Options<'a> {
    pub(super) before: &'a str,
    pub(super) after: &'a str,
    pub(super) workflow: Option<&'a str>,
    pub(super) full: bool,
}

pub(super) fn inputs(root: &Path, options: &Options<'_>) -> Inputs {
    let event = std::env::var("GITHUB_EVENT_NAME").unwrap_or_default();
    resolve_inputs(
        options,
        &event,
        |workflow, after| successful_base(root, workflow, after),
        |before, after| {
            git(
                root,
                &[
                    "diff",
                    "--no-renames",
                    "--name-only",
                    "-z",
                    before,
                    after,
                    "--",
                ],
            )
        },
    )
}

fn resolve_inputs(
    options: &Options<'_>,
    event: &str,
    baseline: impl FnOnce(&str, &str) -> Option<String>,
    diff: impl FnOnce(&str, &str) -> Result<Vec<u8>>,
) -> Inputs {
    let mut inputs = Inputs {
        paths: Vec::new(),
        full: true,
        reason: "manual full validation",
    };
    if options.full || event != "push" {
        return inputs;
    }
    let baseline = options.workflow.map_or_else(
        || Some(options.before.to_owned()),
        |workflow| baseline(workflow, options.after),
    );
    let Some(before) =
        baseline.filter(|value| !value.is_empty() && !value.bytes().all(|b| b == b'0'))
    else {
        inputs.reason = "no successful ancestor baseline";
        return inputs;
    };
    let Ok(raw) = diff(&before, options.after) else {
        inputs.reason = "baseline diff unavailable";
        return inputs;
    };
    // Invalid UTF-8 cannot safely be mapped to a configured owner. Validate all
    // inputs rather than lossy-decoding a filename into an unrelated path.
    let Ok(paths) = raw
        .split(|byte| *byte == 0)
        .filter(|part| !part.is_empty())
        .map(|part| std::str::from_utf8(part).map(str::to_owned))
        .collect()
    else {
        inputs.reason = "baseline diff unavailable";
        return inputs;
    };
    inputs.paths = paths;
    inputs.full = false;
    inputs.reason = "changes since successful validation";
    inputs
}

fn git(root: &Path, arguments: &[&str]) -> Result<Vec<u8>> {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(arguments)
        .current_dir(std::env::temp_dir())
        .output()
        .map_err(|error| error.to_string())?;
    if !output.status.success() {
        return Err("Git baseline command failed.".into());
    }
    Ok(output.stdout)
}

fn valid_repository(repository: &str) -> bool {
    let components: Vec<_> = repository.split('/').collect();
    components.len() == 2
        && components.iter().all(|part| {
            !part.is_empty()
                && part
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || b"_.-".contains(&byte))
        })
}

fn fetch_runs(repository: &str, token: &str, workflow: &str) -> Result<Value> {
    let encoded: String = workflow
        .bytes()
        .map(|byte| {
            if byte.is_ascii_alphanumeric() || b"-._~".contains(&byte) {
                char::from(byte).to_string()
            } else {
                format!("%{byte:02X}")
            }
        })
        .collect();
    let url = format!(
        "https://api.github.com/repos/{repository}/actions/workflows/{encoded}/runs?branch=main&status=success&event=push&per_page=100"
    );
    let config = ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(20)))
        .https_only(true);
    #[cfg(windows)]
    let config = config.tls_config(
        ureq::tls::TlsConfig::builder()
            .provider(ureq::tls::TlsProvider::NativeTls)
            .build(),
    );
    let config = config.build();
    let mut response = ureq::Agent::new_with_config(config)
        .get(&url)
        .header("Authorization", &format!("Bearer {token}"))
        .header("Accept", "application/vnd.github+json")
        .header("User-Agent", "workspace-ci-impact")
        .call()
        .map_err(|_| "Workflow baseline API unavailable.".to_owned())?;
    let body = response
        .body_mut()
        .read_to_string()
        .map_err(|_| "Workflow baseline response unavailable.".to_owned())?;
    serde_json::from_str(&body).map_err(|_| "Malformed workflow baseline response.".into())
}

fn successful_base(root: &Path, workflow: &str, after: &str) -> Option<String> {
    lookup_base(
        &std::env::var("GITHUB_REPOSITORY").unwrap_or_default(),
        &std::env::var("GITHUB_TOKEN").unwrap_or_default(),
        workflow,
        after,
        fetch_runs,
        |candidate, after| git(root, &["merge-base", "--is-ancestor", candidate, after]).is_ok(),
    )
}

fn lookup_base(
    repository: &str,
    token: &str,
    workflow: &str,
    after: &str,
    fetch: impl FnOnce(&str, &str, &str) -> Result<Value>,
    mut ancestor: impl FnMut(&str, &str) -> bool,
) -> Option<String> {
    if !valid_repository(repository) || token.is_empty() {
        return None;
    }
    let response = fetch(repository, token, workflow).ok()?;
    for run in response.get("workflow_runs")?.as_array()? {
        let candidate = run.get("head_sha")?.as_str()?;
        if candidate != after
            && candidate.len() == 40
            && candidate
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
            && ancestor(candidate, after)
        {
            return Some(candidate.to_owned());
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::{Options, lookup_base, resolve_inputs};
    use serde_json::json;

    fn options<'a>(before: &'a str, workflow: Option<&'a str>) -> Options<'a> {
        Options {
            before,
            after: "after",
            workflow,
            full: false,
        }
    }

    #[test]
    fn successful_ancestor_skips_current_invalid_and_unrelated_heads() {
        let [after, unrelated, newest, older] =
            ['a', 'b', 'c', 'd'].map(|c| c.to_string().repeat(40));
        let mut checked = Vec::new();
        let result = lookup_base(
            "owner/project",
            "token",
            "foundation.yml",
            &after,
            |_, _, _| {
                Ok(json!({"workflow_runs": [
                    {"head_sha": after}, {"head_sha": "invalid"}, {"head_sha": unrelated},
                    {"head_sha": newest}, {"head_sha": older}
                ]}))
            },
            |candidate, _| {
                checked.push(candidate.to_owned());
                candidate != unrelated
            },
        );
        assert_eq!(result, Some(newest.clone()));
        assert_eq!(checked, [unrelated, newest]);
    }

    #[test]
    fn unavailable_baseline_never_uses_the_event_parent() {
        let result = resolve_inputs(
            &options("event-parent", Some("foundation.yml")),
            "push",
            |_, _| None,
            |_, _| Err("must not diff".into()),
        );
        assert!(result.full);
        assert_eq!(result.reason, "no successful ancestor baseline");
    }

    #[test]
    fn validated_ancestor_drives_the_diff() {
        let mut seen = Vec::new();
        let result = resolve_inputs(
            &options("event-parent", Some("foundation.yml")),
            "push",
            |_, _| Some("validated".into()),
            |before, after| {
                seen.extend([before.to_owned(), after.to_owned()]);
                Ok(b"leaf/src/lib.rs\0".to_vec())
            },
        );
        assert_eq!(seen, ["validated", "after"]);
        assert!(!result.full);
        assert_eq!(result.paths, ["leaf/src/lib.rs"]);
    }

    #[test]
    fn manual_dispatch_and_explicit_full_skip_baseline_and_diff() {
        for (event, full) in [("workflow_dispatch", false), ("push", true)] {
            let mut arguments = options("parent", Some("foundation.yml"));
            arguments.full = full;
            let result =
                resolve_inputs(&arguments, event, |_, _| None, |_, _| Err("unused".into()));
            assert!(result.full);
            assert_eq!(result.reason, "manual full validation");
        }
    }

    #[test]
    fn direct_baseline_zero_and_failed_or_invalid_diffs_stay_conservative() {
        let result = resolve_inputs(
            &options("parent", None),
            "push",
            |_, _| None,
            |before, _| {
                assert_eq!(before, "parent");
                Ok(b"leaf/src/lib.rs\0".to_vec())
            },
        );
        assert!(!result.full);
        for before in ["", "000000"] {
            assert!(
                resolve_inputs(
                    &options(before, None),
                    "push",
                    |_, _| None,
                    |_, _| Ok(Vec::new())
                )
                .full
            );
        }
        assert!(
            resolve_inputs(
                &options("parent", None),
                "push",
                |_, _| None,
                |_, _| Err("diff failed".into())
            )
            .full
        );
        assert!(
            resolve_inputs(
                &options("parent", None),
                "push",
                |_, _| None,
                |_, _| Ok(vec![255, 0])
            )
            .full
        );
    }

    #[test]
    fn missing_token_bad_repository_or_unavailable_api_has_no_baseline() {
        for (repository, token) in [("owner/project", ""), ("../bad/repo", "token")] {
            assert_eq!(
                lookup_base(
                    repository,
                    token,
                    "foundation.yml",
                    "after",
                    |_, _, _| Err("unused".into()),
                    |_, _| true
                ),
                None
            );
        }
        assert_eq!(
            lookup_base(
                "owner/project",
                "token",
                "foundation.yml",
                "after",
                |_, _, _| Err("unavailable".into()),
                |_, _| true
            ),
            None
        );
    }
}
