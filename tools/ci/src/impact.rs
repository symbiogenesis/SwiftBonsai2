//! Conservative crate-aware CI routing from source manifests and changed paths.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use serde::{Deserialize, Serialize};

#[path = "workspace.rs"]
mod workspace;

/// The complete routing decision consumed by CI jobs and the aggregate gate.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Plan {
    /// Sorted workspace package names, including all transitive consumers.
    pub packages: Vec<String>,
    /// Whether each configured application group is selected.
    pub groups: BTreeMap<String, bool>,
    /// Whether conservative policy requires every package and group.
    pub full: bool,
    /// Sorted, deduplicated repository-relative changed paths.
    pub paths: Vec<String>,
    /// The reason for selecting incremental or complete coverage.
    pub reason: String,
}

#[derive(Default, Deserialize)]
struct Group {
    #[serde(default)]
    packages: BTreeSet<String>,
    #[serde(default)]
    exclude_packages: BTreeSet<String>,
    #[serde(default)]
    groups: BTreeSet<String>,
    #[serde(default)]
    paths: Vec<String>,
    #[serde(default)]
    observes: Vec<String>,
}

#[derive(Deserialize)]
struct Config {
    groups: BTreeMap<String, Group>,
    #[serde(default)]
    shared: Vec<String>,
    #[serde(default)]
    ignored: Vec<String>,
    #[serde(default)]
    package_inputs: BTreeMap<String, Vec<String>>,
    #[serde(default)]
    package_paths: BTreeMap<String, Vec<String>>,
}

#[derive(Clone, Copy)]
enum Token {
    Literal(char),
    Character,
    Segment,
    Anything,
    Directories,
}

fn tokens(pattern: &str) -> Vec<Token> {
    let characters: Vec<_> = pattern.chars().collect();
    let mut result = Vec::new();
    let mut index = 0;
    while index < characters.len() {
        if characters[index..].starts_with(&['*', '*', '/']) {
            result.push(Token::Directories);
            index += 3;
        } else if characters[index..].starts_with(&['*', '*']) {
            result.push(Token::Anything);
            index += 2;
        } else {
            result.push(match characters[index] {
                '*' => Token::Segment,
                '?' => Token::Character,
                character => Token::Literal(character),
            });
            index += 1;
        }
    }
    result
}

/// Match repository-relative globs, with a single `*` confined to one segment.
///
/// `**/` also matches zero directories. Other punctuation is literal, and `?`
/// matches one Unicode character other than `/`.
#[must_use]
pub fn matches(path: &str, pattern: &str) -> bool {
    let characters: Vec<_> = path.chars().collect();
    let mut previous = vec![false; characters.len() + 1];
    previous[0] = true;
    for token in tokens(pattern) {
        let mut next = vec![false; previous.len()];
        if matches!(token, Token::Segment | Token::Anything | Token::Directories) {
            next[0] = previous[0];
        }
        let mut directory_start = false;
        for (index, &character) in characters.iter().enumerate() {
            next[index + 1] = match token {
                Token::Literal(expected) => previous[index] && character == expected,
                Token::Character => previous[index] && character != '/',
                Token::Segment => previous[index + 1] || (next[index] && character != '/'),
                Token::Anything => previous[index + 1] || (next[index] && character != '\n'),
                Token::Directories => {
                    directory_start |= previous[index];
                    directory_start &= character != '\n';
                    previous[index + 1] || (directory_start && character == '/')
                }
            };
        }
        previous = next;
    }
    previous[characters.len()]
}

fn any_match(path: &str, patterns: &[String]) -> bool {
    patterns.iter().any(|pattern| matches(path, pattern))
}

fn validate(config: &Config, workspace: &workspace::Workspace) -> crate::Result<()> {
    for group in config.groups.values() {
        if group
            .packages
            .iter()
            .any(|name| name != "*" && !workspace.packages.contains_key(name))
        {
            return Err("CI group references an unknown workspace package".into());
        }
        if group
            .groups
            .iter()
            .any(|name| !config.groups.contains_key(name))
        {
            return Err("CI group references an unknown group".into());
        }
    }
    if config
        .package_inputs
        .keys()
        .chain(config.package_paths.keys())
        .any(|name| !workspace.packages.contains_key(name))
    {
        return Err("CI input map references an unknown workspace package".into());
    }
    Ok(())
}

fn is_owned(path: &str, directory: &str, patterns: Option<&Vec<String>>) -> bool {
    patterns.map_or_else(
        || {
            if directory == "." {
                [
                    "src/**",
                    "tests/**/*.rs",
                    "examples/**",
                    "benches/**",
                    "build.rs",
                ]
                .iter()
                .any(|pattern| matches(path, pattern))
            } else {
                path.strip_prefix(directory)
                    .is_some_and(|suffix| suffix.starts_with('/'))
            }
        },
        |patterns| any_match(path, patterns),
    )
}

fn select_groups(
    config: &Config,
    affected: &BTreeSet<String>,
    selected: &mut BTreeMap<String, bool>,
) {
    for (name, group) in &config.groups {
        let relevant = affected.difference(&group.exclude_packages);
        let package_selected = relevant
            .into_iter()
            .any(|package| group.packages.contains("*") || group.packages.contains(package));
        if package_selected {
            selected.insert(name.clone(), true);
        }
    }
    for _ in &config.groups {
        for (name, group) in &config.groups {
            if group
                .groups
                .iter()
                .any(|dependency| selected.get(dependency) == Some(&true))
            {
                selected.insert(name.clone(), true);
            }
        }
    }
}

/// Select affected workspace packages and groups without resolving dependencies.
///
/// Shared inputs, added or removed manifests and unclassified changes require
/// complete coverage. A lint observer alone does not establish an input owner.
///
/// # Errors
/// Returns a diagnostic for malformed configuration, invalid manifests or an
/// unreadable workspace. Unknown configured package/group names are rejected.
#[allow(
    clippy::too_many_lines,
    reason = "The ordered routing policy follows the existing planner's conservative fallback precedence."
)]
pub fn select(
    root: &Path,
    paths: &[String],
    mut full: bool,
    config: &serde_json::Value,
    reason: &str,
) -> crate::Result<Plan> {
    let config: Config =
        serde_json::from_value(config.clone()).map_err(|error| error.to_string())?;
    let root = root.canonicalize().map_err(|error| error.to_string())?;
    let workspace = workspace::read(&root)?;
    validate(&config, &workspace)?;
    let paths: Vec<_> = paths
        .iter()
        .cloned()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    let mut affected = BTreeSet::new();
    let mut selected: BTreeMap<_, _> = config
        .groups
        .keys()
        .map(|name| (name.clone(), false))
        .collect();
    let mut reason = reason.to_owned();
    let manifests: BTreeSet<_> = workspace
        .packages
        .values()
        .map(|package| {
            if package.directory == "." {
                "Cargo.toml".to_owned()
            } else {
                format!("{}/Cargo.toml", package.directory)
            }
        })
        .collect();
    let mut owners: Vec<_> = workspace.packages.iter().collect();
    owners.sort_by_key(|(_, package)| std::cmp::Reverse(package.path.components().count()));
    for path in &paths {
        if any_match(path, &config.shared) {
            full = true;
            reason = "shared build or CI input".into();
        }
        if (path == "Cargo.toml" || path.ends_with("/Cargo.toml")) && !manifests.contains(path) {
            full = true;
            reason = "added or removed workspace manifest".into();
        }
        let mut known = false;
        for (name, package) in &owners {
            if is_owned(path, &package.directory, config.package_paths.get(*name)) {
                affected.insert((*name).clone());
                known = true;
                break;
            }
        }
        for (name, patterns) in &config.package_inputs {
            if any_match(path, patterns) {
                affected.insert(name.clone());
                known = true;
            }
        }
        for (name, group) in &config.groups {
            if any_match(path, &group.paths) {
                selected.insert(name.clone(), true);
                known = true;
            }
            if any_match(path, &group.observes) {
                selected.insert(name.clone(), true);
            }
        }
        if !known && !any_match(path, &config.ignored) {
            full = true;
            reason = "unclassified input (conservative full validation)".into();
        }
    }
    let mut pending: Vec<_> = affected.iter().cloned().collect();
    while let Some(package) = pending.pop() {
        if let Some(consumers) = workspace.reverse.get(&package) {
            for consumer in consumers {
                if affected.insert(consumer.clone()) {
                    pending.push(consumer.clone());
                }
            }
        }
    }
    select_groups(&config, &affected, &mut selected);
    if full {
        affected = workspace.packages.into_keys().collect();
        for value in selected.values_mut() {
            *value = true;
        }
    }
    Ok(Plan {
        packages: affected.into_iter().collect(),
        groups: selected,
        full,
        paths,
        reason,
    })
}
