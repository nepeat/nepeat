//! The private files of a `NetBotz` target and the askpass helper.
//!
//! The password never enters argv, a log line or an error message: ssh asks for it through
//! `SSH_ASKPASS`, which is this same binary started with `LAB_POWER_ASKPASS_CREDENTIALS` naming the
//! credentials file; in that mode it prints the password to ssh and exits.

use std::fs;
use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use serde_json::Value;

/// Set (by lab-power, for ssh) to the credentials file: run as the askpass helper.
pub(crate) const ASKPASS_ENV: &str = "LAB_POWER_ASKPASS_CREDENTIALS";

/// Refuses anything but a regular file that only its owner can read or write.
pub(crate) fn check_private(path: &Path, what: &str) -> Result<(), String> {
    let meta = fs::metadata(path).map_err(|e| format!("cannot read the {what} {}: {}", path.display(), e.kind()))?;
    if !meta.is_file() {
        return Err(format!("the {what} {} is not a regular file", path.display()));
    }
    let mode = meta.permissions().mode() & 0o777;
    if mode & 0o077 != 0 {
        return Err(format!(
            "refusing the {what} {}: mode {mode:03o} lets group or others read or write it (chmod 600 it)",
            path.display()
        ));
    }
    Ok(())
}

/// The path in environment variable `var` (set in .envrc, never in the repo's config).
pub(crate) fn path_from_env(var: &str, what: &str) -> Result<PathBuf, String> {
    let path = std::env::var_os(var)
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .ok_or_else(|| format!("set {var} to the {what} (see tools/README.md)"))?;
    if !path.is_absolute() {
        return Err(format!("{var} must be an absolute path"));
    }
    // ssh splits option values on whitespace, and its config parser treats some characters specially.
    if path
        .to_str()
        .is_none_or(|p| p.contains(|c: char| c.is_whitespace() || "\"'%#\\=".contains(c)))
    {
        return Err(format!(
            "{var} names a path ssh cannot take literally (spaces, quotes, %, #, = or non-UTF-8)"
        ));
    }
    Ok(path)
}

/// Checks the credentials file and returns the user name; the password is checked, then dropped.
pub(crate) fn username(path: &Path) -> Result<String, String> {
    check_private(path, "credentials file")?;
    let creds = read_json(path).ok_or_else(|| format!("the credentials file {} is not valid JSON", path.display()))?;
    let user = creds.get("username").and_then(Value::as_str).unwrap_or_default();
    if user.is_empty() || user.starts_with('-') || !user.chars().all(|c| c.is_ascii_alphanumeric() || "._-".contains(c))
    {
        return Err(format!(
            "the credentials file {} has no usable \"username\"",
            path.display()
        ));
    }
    if password(&creds).is_none() {
        return Err(format!(
            "the credentials file {} has no usable \"password\"",
            path.display()
        ));
    }
    Ok(user.to_owned())
}

fn read_json(path: &Path) -> Option<Value> {
    serde_json::from_slice(&fs::read(path).ok()?).ok()
}

fn password(creds: &Value) -> Option<&str> {
    creds
        .get("password")
        .and_then(Value::as_str)
        .filter(|p| !p.is_empty() && !p.contains(['\r', '\n', '\0']))
}

/// Askpass mode: when ssh runs us with `ASKPASS_ENV` set, print the password for a password
/// prompt (and nothing for any other prompt, such as a host-key question), then exit.
pub(crate) fn askpass_mode() -> Option<ExitCode> {
    let path = PathBuf::from(std::env::var_os(ASKPASS_ENV)?);
    let prompt = std::env::args().nth(1).unwrap_or_default().to_ascii_lowercase();
    if !prompt.contains("password") || prompt.contains("yes/no") || std::env::args().count() != 2 {
        return Some(ExitCode::FAILURE);
    }
    if check_private(&path, "credentials file").is_err() {
        return Some(ExitCode::FAILURE);
    }
    let Some(creds) = read_json(&path) else {
        return Some(ExitCode::FAILURE);
    };
    let Some(pw) = password(&creds) else {
        return Some(ExitCode::FAILURE);
    };
    let mut out = std::io::stdout().lock();
    if writeln!(out, "{pw}").and_then(|()| out.flush()).is_err() {
        return Some(ExitCode::FAILURE);
    }
    Some(ExitCode::SUCCESS)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn group_or_world_access_refused() {
        let dir = std::env::temp_dir().join(format!("lab-power-secure-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let f = dir.join("c.json");
        fs::write(&f, br#"{"username": "apc", "password": "pw"}"#).unwrap();
        for (mode, ok) in [
            (0o600, true),
            (0o400, true),
            (0o640, false),
            (0o604, false),
            (0o620, false),
        ] {
            fs::set_permissions(&f, fs::Permissions::from_mode(mode)).unwrap();
            assert_eq!(check_private(&f, "x").is_ok(), ok, "{mode:o}");
        }
        fs::set_permissions(&f, fs::Permissions::from_mode(0o600)).unwrap();
        assert_eq!(username(&f).unwrap(), "apc");
        fs::write(&f, br#"{"username": "-oProxyCommand", "password": "pw"}"#).unwrap();
        assert!(username(&f).is_err());
        fs::write(&f, br#"{"username": "apc", "password": "a\nb"}"#).unwrap();
        let err = username(&f).unwrap_err();
        assert!(!err.contains("a\nb"));
        assert!(check_private(&dir, "x").is_err());
        fs::remove_dir_all(&dir).unwrap();
    }
}
