//! Command parsing and GitHub Actions output contracts.

use std::ffi::OsString;
use std::fs::{self, OpenOptions};
use std::io::Write as _;
use std::path::PathBuf;

use serde_json::Value;

use crate::{Result, gate, impact};

#[path = "planning.rs"]
mod planning;

const USAGE: &str = "Usage: bonsai-ci impact [--root PATH] [--config PATH] [--workflow FILE] [--before REV] [--after REV] [--full]\n       bonsai-ci gate [--root PATH] [--config PATH] --workflow NAME";

#[derive(Default)]
struct Arguments {
    command: String,
    root: Option<PathBuf>,
    config: Option<PathBuf>,
    workflow: Option<String>,
    before: String,
    after: String,
    full: bool,
}

fn parse(arguments: &[OsString]) -> Result<Arguments> {
    let mut values = arguments.iter();
    let command = values
        .next()
        .and_then(|value| value.to_str())
        .ok_or(USAGE)?;
    if !["impact", "gate"].contains(&command) {
        return Err(USAGE.into());
    }
    let mut parsed = Arguments {
        command: command.into(),
        after: "HEAD".into(),
        ..Arguments::default()
    };
    while let Some(option) = values.next() {
        if option == "--full" && command == "impact" {
            parsed.full = true;
            continue;
        }
        let option = option.to_str().ok_or(USAGE)?;
        if !["--root", "--config", "--workflow", "--before", "--after"].contains(&option)
            || (command == "gate" && ["--before", "--after"].contains(&option))
        {
            return Err(USAGE.into());
        }
        let value = values.next().ok_or(USAGE)?;
        match option {
            "--root" => parsed.root = Some(value.into()),
            "--config" => parsed.config = Some(value.into()),
            "--workflow" => parsed.workflow = Some(value.to_str().ok_or(USAGE)?.into()),
            "--before" => parsed.before = value.to_str().ok_or(USAGE)?.into(),
            "--after" => parsed.after = value.to_str().ok_or(USAGE)?.into(),
            _ => return Err(USAGE.into()),
        }
    }
    if command == "gate" && parsed.workflow.as_ref().is_none_or(String::is_empty) {
        return Err(USAGE.into());
    }
    Ok(parsed)
}

fn append_environment_file(variable: &str, contents: &str) -> Result<()> {
    if let Some(path) = std::env::var_os(variable).filter(|path| !path.is_empty()) {
        OpenOptions::new()
            .append(true)
            .create(true)
            .open(path)
            .and_then(|mut file| file.write_all(contents.as_bytes()))
            .map_err(|error| format!("Cannot append {variable}: {error}"))?;
    }
    Ok(())
}

fn action_outputs(plan: &impact::Plan, config: &Value, serialized: &str) -> Result<String> {
    use std::fmt::Write as _;

    let mut output = format!(
        "plan={serialized}\npackages={}\n",
        serde_json::to_string(&plan.packages).map_err(|e| e.to_string())?
    );
    for (name, selected) in &plan.groups {
        writeln!(output, "{name}={selected}").map_err(|error| error.to_string())?;
    }
    if let Some(matrices) = config.get("matrices") {
        for (name, choices) in matrices
            .as_object()
            .ok_or("CI matrices must be an object.")?
        {
            let mut values = Vec::new();
            for (choice, group) in choices
                .as_object()
                .ok_or("CI matrix choices must be an object.")?
            {
                let selected = group
                    .as_str()
                    .and_then(|name| plan.groups.get(name))
                    .ok_or("CI matrix references an unknown group.")?;
                if *selected {
                    values.push(choice);
                }
            }
            writeln!(
                output,
                "{name}={}",
                serde_json::to_string(&values).map_err(|e| e.to_string())?
            )
            .map_err(|error| error.to_string())?;
        }
    }
    Ok(output)
}

/// Execute `impact` or `gate`, returning the command's standard output.
///
/// # Errors
/// Rejects invalid arguments/configuration, failed output writes and incomplete
/// or unsuccessful selected gates. Baseline failures select full coverage.
pub fn run(arguments: &[OsString]) -> Result<String> {
    if arguments.len() == 1 && arguments[0] == "--help" {
        return Ok(USAGE.into());
    }
    let arguments = parse(arguments)?;
    let root = arguments
        .root
        .or_else(|| std::env::var_os("GITHUB_WORKSPACE").map(PathBuf::from))
        .map_or_else(std::env::current_dir, Ok)
        .and_then(fs::canonicalize)
        .map_err(|error| error.to_string())?;
    let config_path = arguments
        .config
        .unwrap_or_else(|| root.join(".github/ci/impact.json"));
    let config: Value =
        serde_json::from_slice(&fs::read(config_path).map_err(|error| error.to_string())?)
            .map_err(|error| error.to_string())?;
    if arguments.command == "gate" {
        let workflow = arguments.workflow.ok_or(USAGE)?;
        let jobs = config
            .get("workflows")
            .and_then(|workflows| workflows.get(&workflow))
            .ok_or("Unknown CI workflow.")?;
        let needs: Value =
            serde_json::from_str(&std::env::var("CI_NEEDS").map_err(|error| error.to_string())?)
                .map_err(|error| error.to_string())?;
        let errors = gate::failures(&needs, jobs);
        return if errors.is_empty() {
            Ok("All selected CI groups passed; unselected groups were skipped.".into())
        } else {
            Err(errors.join("\n"))
        };
    }
    let inputs = planning::inputs(
        &root,
        &planning::Options {
            before: &arguments.before,
            after: &arguments.after,
            workflow: arguments.workflow.as_deref(),
            full: arguments.full,
        },
    );
    let plan = impact::select(&root, &inputs.paths, inputs.full, &config, inputs.reason)?;
    let serialized = serde_json::to_string(&plan).map_err(|error| error.to_string())?;
    let outputs = action_outputs(&plan, &config, &serialized)?;
    let groups: Vec<_> = plan
        .groups
        .iter()
        .filter(|(_, selected)| **selected)
        .map(|(name, _)| name.as_str())
        .collect();
    let summary = format!(
        "Affected crates: {}\n\nSelected groups: {}\n\nSelection: {}\n",
        if plan.packages.is_empty() {
            "none".into()
        } else {
            plan.packages.join(", ")
        },
        if groups.is_empty() {
            "none".into()
        } else {
            groups.join(", ")
        },
        plan.reason
    );
    append_environment_file("GITHUB_OUTPUT", &outputs)?;
    append_environment_file("GITHUB_STEP_SUMMARY", &summary)?;
    Ok(serialized)
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use serde_json::json;

    use super::{action_outputs, parse};
    use crate::impact::Plan;

    #[test]
    fn output_preserves_plan_packages_booleans_and_filtered_matrices() -> crate::Result<()> {
        let plan = Plan {
            packages: vec!["leaf".into()],
            groups: BTreeMap::from([("rust".into(), true), ("scripts".into(), false)]),
            full: false,
            paths: vec!["leaf/src/lib.rs".into()],
            reason: "changed inputs".into(),
        };
        let serialized = serde_json::to_string(&plan).map_err(|e| e.to_string())?;
        let output = action_outputs(
            &plan,
            &json!({"matrices": {"lanes": {"core":"rust", "scripts":"scripts"}}}),
            &serialized,
        )?;
        assert!(output.contains(&format!("plan={serialized}\n")));
        assert!(
            output.contains("packages=[\"leaf\"]\nrust=true\nscripts=false\nlanes=[\"core\"]\n")
        );
        assert!(
            action_outputs(
                &plan,
                &json!({"matrices":{"lanes":{"bad":"missing"}}}),
                &serialized
            )
            .is_err()
        );
        Ok(())
    }

    #[test]
    fn command_arguments_reject_unknown_options_and_incomplete_gate() {
        for arguments in [
            vec![],
            vec!["gate"],
            vec!["impact", "--unknown"],
            vec!["impact", "--after"],
            vec!["gate", "--workflow", "foundation", "--full"],
        ] {
            assert!(parse(&arguments.into_iter().map(Into::into).collect::<Vec<_>>()).is_err());
        }
        assert!(parse(&["impact".into(), "--full".into()]).is_ok());
        assert!(parse(&["gate".into(), "--workflow".into(), "foundation".into()]).is_ok());
    }
}
