//! Source-only Cargo workspace graph discovery.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use toml::Value;

pub(super) struct Package {
    pub(super) path: PathBuf,
    pub(super) directory: String,
    manifest: Value,
}

pub(super) struct Workspace {
    pub(super) packages: BTreeMap<String, Package>,
    pub(super) reverse: BTreeMap<String, BTreeSet<String>>,
}

fn manifest(path: &Path) -> crate::Result<Value> {
    let text =
        std::fs::read_to_string(path).map_err(|error| format!("{}: {error}", path.display()))?;
    toml::from_str(&text).map_err(|error| format!("{}: {error}", path.display()))
}

fn strings<'a>(value: Option<&'a Value>, name: &str) -> crate::Result<Vec<&'a str>> {
    let Some(value) = value else {
        return Ok(Vec::new());
    };
    value
        .as_array()
        .ok_or_else(|| format!("{name} must be an array"))?
        .iter()
        .map(|item| {
            item.as_str()
                .ok_or_else(|| format!("{name} must contain strings"))
        })
        .collect()
}

fn relative(root: &Path, directory: &Path) -> crate::Result<String> {
    let path = directory
        .strip_prefix(root)
        .map_err(|error| error.to_string())?;
    if path.as_os_str().is_empty() {
        return Ok(".".into());
    }
    path.to_str()
        .map(|text| text.replace('\\', "/"))
        .ok_or_else(|| "Workspace path is not UTF-8".into())
}

fn excluded(root: &Path, directory: &Path, excludes: &[&str]) -> crate::Result<bool> {
    let relative = relative(root, directory)?;
    Ok(excludes
        .iter()
        .any(|pattern| super::matches(&relative, pattern)))
}

fn class_matches(character: char, pattern: &[char]) -> bool {
    let (negative, pattern) = if pattern.first() == Some(&'!') {
        (true, &pattern[1..])
    } else {
        (false, pattern)
    };
    let mut found = false;
    let mut index = 0;
    while index < pattern.len() {
        if index + 2 < pattern.len() && pattern[index + 1] == '-' {
            found |= (pattern[index]..=pattern[index + 2]).contains(&character);
            index += 3;
        } else {
            found |= pattern[index] == character;
            index += 1;
        }
    }
    found != negative
}

fn member_matches(name: &str, pattern: &str) -> bool {
    let name: Vec<_> = name.chars().collect();
    let pattern: Vec<_> = pattern.chars().collect();
    let mut previous = vec![false; name.len() + 1];
    previous[0] = true;
    let mut index = 0;
    while index < pattern.len() {
        let character = pattern[index];
        let class_end = if character == '[' {
            let start = index + 1 + usize::from(pattern.get(index + 1) == Some(&'!'));
            let start = start + usize::from(pattern.get(start) == Some(&']'));
            pattern.get(start..).and_then(|suffix| {
                suffix
                    .iter()
                    .position(|item| *item == ']')
                    .map(|offset| start + offset)
            })
        } else {
            None
        };
        let mut next = vec![false; previous.len()];
        next[0] = character == '*' && previous[0];
        for (position, &actual) in name.iter().enumerate() {
            next[position + 1] = match (character, class_end) {
                ('*', _) => previous[position + 1] || next[position],
                ('?', _) => previous[position],
                ('[', Some(end)) => {
                    previous[position] && class_matches(actual, &pattern[index + 1..end])
                }
                _ => previous[position] && actual == character,
            };
        }
        index = class_end.map_or(index + 1, |end| end + 1);
        previous = next;
    }
    previous[name.len()]
}

fn children(directory: &Path) -> crate::Result<Vec<PathBuf>> {
    let mut result = Vec::new();
    for entry in
        std::fs::read_dir(directory).map_err(|error| format!("{}: {error}", directory.display()))?
    {
        let entry = entry.map_err(|error| error.to_string())?;
        if entry.path().is_dir() {
            result.push(entry.path());
        }
    }
    result.sort();
    Ok(result)
}

fn expand(directory: &Path, parts: &[&str], output: &mut BTreeSet<PathBuf>) -> crate::Result<()> {
    let Some((part, remaining)) = parts.split_first() else {
        if directory.exists() {
            output.insert(directory.to_path_buf());
        }
        return Ok(());
    };
    if *part == "**" {
        expand(directory, remaining, output)?;
        if directory.is_dir() {
            for child in children(directory)? {
                // Path.glob does not descend through symbolic-link directories.
                if !child
                    .symlink_metadata()
                    .map_err(|error| error.to_string())?
                    .file_type()
                    .is_symlink()
                {
                    expand(&child, parts, output)?;
                }
            }
        }
    } else if part.contains(['*', '?', '[']) {
        if directory.is_dir() {
            for child in children(directory)? {
                if child
                    .file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| member_matches(name, part))
                {
                    expand(&child, remaining, output)?;
                }
            }
        }
    } else {
        expand(&directory.join(part), remaining, output)?;
    }
    Ok(())
}

fn dependencies(
    data: &Value,
    base: &Path,
    root: &Path,
    settings: Option<&Value>,
) -> crate::Result<Vec<PathBuf>> {
    let mut tables = vec![data];
    if let Some(targets) = data.get("target").and_then(Value::as_table) {
        tables.extend(targets.values());
    }
    let mut result = Vec::new();
    for table in tables {
        for kind in ["dependencies", "build-dependencies", "dev-dependencies"] {
            let Some(dependencies) = table.get(kind).and_then(Value::as_table) else {
                continue;
            };
            for (alias, dependency) in dependencies {
                if !dependency.is_table() {
                    continue;
                }
                let (dependency, directory) =
                    if dependency.get("workspace").and_then(Value::as_bool) == Some(true) {
                        (
                            settings
                                .and_then(|settings| settings.get("dependencies"))
                                .and_then(|dependencies| dependencies.get(alias))
                                .ok_or_else(|| format!("Missing workspace dependency {alias}"))?,
                            root,
                        )
                    } else {
                        (dependency, base)
                    };
                if let Some(path) = dependency.get("path") {
                    let path = path
                        .as_str()
                        .ok_or_else(|| format!("Dependency {alias} path must be a string"))?;
                    let directory = directory.join(path);
                    result.push(resolve(&directory)?);
                }
            }
        }
    }
    Ok(result)
}

fn resolve(path: &Path) -> crate::Result<PathBuf> {
    let mut result = PathBuf::new();
    for component in path.components() {
        match component {
            std::path::Component::CurDir => (),
            std::path::Component::ParentDir => {
                result.pop();
            }
            component => {
                result.push(component.as_os_str());
                if result.exists() {
                    result = result.canonicalize().map_err(|error| error.to_string())?;
                }
            }
        }
    }
    Ok(result)
}

pub(super) fn read(root: &Path) -> crate::Result<Workspace> {
    let root_manifest = manifest(&root.join("Cargo.toml"))?;
    let settings = root_manifest.get("workspace");
    let excludes = strings(
        settings.and_then(|value| value.get("exclude")),
        "workspace.exclude",
    )?;
    let mut manifests = Vec::new();
    if root_manifest.get("package").is_some() {
        manifests.push(root.join("Cargo.toml"));
    }
    for pattern in strings(
        settings.and_then(|value| value.get("members")),
        "workspace.members",
    )? {
        if Path::new(pattern).is_absolute() {
            return Err("Workspace members must be relative paths".into());
        }
        let mut directories = BTreeSet::new();
        expand(
            root,
            &pattern.split('/').collect::<Vec<_>>(),
            &mut directories,
        )?;
        for directory in directories {
            let directory = directory
                .canonicalize()
                .map_err(|error| error.to_string())?;
            if !excluded(root, &directory, &excludes)? {
                let candidate = directory.join("Cargo.toml");
                if !manifests.contains(&candidate) {
                    manifests.push(candidate);
                }
            }
        }
    }
    let mut packages = BTreeMap::new();
    let mut index = 0;
    while let Some(path) = manifests.get(index).cloned() {
        let data = manifest(&path)?;
        let name = data
            .get("package")
            .and_then(|package| package.get("name"))
            .and_then(Value::as_str)
            .ok_or_else(|| format!("{} has no package.name", path.display()))?
            .to_owned();
        if packages.contains_key(&name) {
            return Err(format!("Duplicate workspace package: {name}"));
        }
        let base = path
            .parent()
            .ok_or_else(|| "Manifest has no parent directory".to_owned())?;
        for directory in dependencies(&data, base, root, settings)? {
            if directory.starts_with(root) && !excluded(root, &directory, &excludes)? {
                let candidate = directory.join("Cargo.toml");
                if !manifests.contains(&candidate) {
                    manifests.push(candidate);
                }
            }
        }
        packages.insert(
            name,
            Package {
                path: base.to_path_buf(),
                directory: relative(root, base)?,
                manifest: data,
            },
        );
        index += 1;
    }
    let by_directory: BTreeMap<_, _> = packages
        .iter()
        .map(|(name, package)| (&package.path, name))
        .collect();
    let mut reverse: BTreeMap<_, BTreeSet<String>> = packages
        .keys()
        .map(|name| (name.clone(), BTreeSet::new()))
        .collect();
    for (name, package) in &packages {
        for directory in dependencies(&package.manifest, &package.path, root, settings)? {
            if let Some(dependency) = by_directory.get(&directory) {
                reverse
                    .entry((*dependency).clone())
                    .or_default()
                    .insert(name.clone());
            }
        }
    }
    Ok(Workspace { packages, reverse })
}
