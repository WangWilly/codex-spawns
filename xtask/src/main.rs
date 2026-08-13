use std::env;
use std::fs;
use std::path::Path;
use std::process::{self, Command};

fn main() {
    if let Err(error) = run(env::args().skip(1)) {
        eprintln!("error: {error}");
        process::exit(1);
    }
}

fn run(args: impl Iterator<Item = String>) -> Result<(), String> {
    let mut args = args;
    match args.next().as_deref() {
        Some("release-version") => {}
        _ => return Err(usage()),
    }

    let version = args.next().ok_or_else(usage)?;
    let mut dry_run = false;
    for argument in args {
        match argument.as_str() {
            "--dry-run" => dry_run = true,
            _ => return Err(format!("unknown argument `{argument}`\n{}", usage())),
        }
    }
    validate_semver(&version)?;

    let root = env::current_dir().map_err(|error| format!("read working directory: {error}"))?;
    let manifest_path = root.join("Cargo.toml");
    let lock_path = root.join("Cargo.lock");
    let manifest = fs::read_to_string(&manifest_path)
        .map_err(|error| format!("read {}: {error}", manifest_path.display()))?;
    let old_version = package_version(&manifest)?;
    let updated_manifest = replace_package_version(&manifest, &old_version, &version)?;

    if dry_run {
        println!("Would update codex-spawns from {old_version} to {version}");
        return Ok(());
    }

    let original_lock =
        fs::read(&lock_path).map_err(|error| format!("read {}: {error}", lock_path.display()))?;
    fs::write(&manifest_path, updated_manifest)
        .map_err(|error| format!("write {}: {error}", manifest_path.display()))?;

    let result = update_and_verify(&root, &lock_path, &version);
    if let Err(error) = result {
        let manifest_restore = fs::write(&manifest_path, manifest);
        let lock_restore = fs::write(&lock_path, original_lock);
        if manifest_restore.is_err() || lock_restore.is_err() {
            return Err(format!(
                "{error}; additionally failed to restore original files"
            ));
        }
        return Err(error);
    }

    println!("Updated codex-spawns to {version}");
    println!("Next: review Cargo.toml and Cargo.lock, then commit and tag v{version}.");
    Ok(())
}

fn update_and_verify(root: &Path, lock_path: &Path, version: &str) -> Result<(), String> {
    let check = Command::new("cargo")
        .arg("check")
        .current_dir(root)
        .status()
        .map_err(|error| format!("run cargo check: {error}"))?;
    if !check.success() {
        return Err("cargo check failed".to_owned());
    }

    let lock = fs::read_to_string(lock_path)
        .map_err(|error| format!("read updated {}: {error}", lock_path.display()))?;
    if !lock_contains_package_version(&lock, "codex-spawns", version) {
        return Err(format!(
            "Cargo.lock was not synchronized to codex-spawns {version}"
        ));
    }

    let output = Command::new("cargo")
        .args(["run", "--quiet", "--", "--version"])
        .current_dir(root)
        .output()
        .map_err(|error| format!("run binary version verification: {error}"))?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    let expected = format!("codex-spawns {version}");
    if !output.status.success() || stdout.trim() != expected {
        return Err(format!(
            "binary version verification failed: expected `{expected}`, got `{}`",
            stdout.trim()
        ));
    }
    Ok(())
}

fn usage() -> String {
    "usage: cargo xtask release-version <SEMVER> [--dry-run]".to_owned()
}

fn validate_semver(version: &str) -> Result<(), String> {
    if version.starts_with('v') || version.trim() != version {
        return Err(format!(
            "`{version}` is not a valid SemVer (omit the `v` prefix)"
        ));
    }
    let (core_and_pre, build) = split_once_optional(version, '+', true)?;
    validate_identifiers(build, true, version)?;
    let (core, pre) = split_once_optional(core_and_pre, '-', false)?;
    validate_identifiers(pre, false, version)?;
    let parts: Vec<_> = core.split('.').collect();
    if parts.len() != 3 || parts.iter().any(|part| !valid_number(part)) {
        return Err(format!("`{version}` is not a valid SemVer"));
    }
    Ok(())
}

fn split_once_optional(
    value: &str,
    separator: char,
    reject_multiple: bool,
) -> Result<(&str, Option<&str>), String> {
    let Some((first, second)) = value.split_once(separator) else {
        return Ok((value, None));
    };
    if reject_multiple && second.contains(separator) {
        return Err(format!("`{value}` is not a valid SemVer"));
    }
    Ok((first, Some(second)))
}

fn validate_identifiers(identifiers: Option<&str>, build: bool, whole: &str) -> Result<(), String> {
    let Some(identifiers) = identifiers else {
        return Ok(());
    };
    if identifiers.is_empty()
        || identifiers.split('.').any(|item| {
            item.is_empty()
                || !item
                    .chars()
                    .all(|character| character.is_ascii_alphanumeric() || character == '-')
                || (!build
                    && item.chars().all(|character| character.is_ascii_digit())
                    && !valid_number(item))
        })
    {
        return Err(format!("`{whole}` is not a valid SemVer"));
    }
    Ok(())
}

fn valid_number(value: &str) -> bool {
    !value.is_empty()
        && value.chars().all(|character| character.is_ascii_digit())
        && (value == "0" || !value.starts_with('0'))
}

fn package_version(manifest: &str) -> Result<String, String> {
    let package = manifest
        .split_once("[package]")
        .map(|(_, rest)| rest)
        .ok_or_else(|| "Cargo.toml has no [package] section".to_owned())?;
    let section = package.split("\n[").next().unwrap_or(package);
    section
        .lines()
        .find_map(|line| quoted_assignment(line, "version"))
        .map(str::to_owned)
        .ok_or_else(|| "Cargo.toml [package] has no string version".to_owned())
}

fn replace_package_version(
    manifest: &str,
    old_version: &str,
    new_version: &str,
) -> Result<String, String> {
    let marker = format!("version = \"{old_version}\"");
    let package_start = manifest
        .find("[package]")
        .ok_or_else(|| "Cargo.toml has no [package] section".to_owned())?;
    let relative = manifest[package_start..]
        .find(&marker)
        .ok_or_else(|| "could not locate package version assignment".to_owned())?;
    let start = package_start + relative;
    let mut updated = manifest.to_owned();
    updated.replace_range(
        start..start + marker.len(),
        &format!("version = \"{new_version}\""),
    );
    Ok(updated)
}

fn quoted_assignment<'a>(line: &'a str, key: &str) -> Option<&'a str> {
    let (left, right) = line.split_once('=')?;
    if left.trim() != key {
        return None;
    }
    right.trim().strip_prefix('"')?.strip_suffix('"')
}

fn lock_contains_package_version(lock: &str, package: &str, version: &str) -> bool {
    lock.split("[[package]]").any(|entry| {
        entry
            .lines()
            .any(|line| quoted_assignment(line, "name") == Some(package))
            && entry
                .lines()
                .any(|line| quoted_assignment(line, "version") == Some(version))
    }) || (lock
        .lines()
        .any(|line| quoted_assignment(line, "name") == Some(package))
        && lock
            .lines()
            .any(|line| quoted_assignment(line, "version") == Some(version)))
}
