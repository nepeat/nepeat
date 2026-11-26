//! The target table: target name -> backend + parameters, and devices as lists of feeds.

use std::collections::BTreeMap;
use std::time::Duration;

use serde_json::{Map, Value};

use crate::cli::ACTIONS;

const BUILTIN: &str = include_str!("../targets.json");
/// Names a JSON file that replaces the built-in table (tests, other labs).
pub(crate) const CONFIG_ENV: &str = "LAB_POWER_CONFIG";
const DEFAULT_CYCLE_OFF: Duration = Duration::from_secs(10);

/// How a target is switched.
#[derive(Debug, Clone)]
pub(crate) enum Backend {
    /// A Home Assistant switch entity; `power_sensor` is an optional W sensor of the same plug.
    HomeAssistant {
        entity: String,
        power_sensor: Option<String>,
    },
    /// The one switched outlet of an APC `NetBotz` 250, through its SSH CLI (`nboutlet`).
    NetBotz(NetBotz),
}

/// Parameters of a `NetBotz` target. The secrets stay in files named by environment variables.
#[derive(Debug, Clone)]
pub(crate) struct NetBotz {
    pub(crate) host: String,
    pub(crate) port: u16,
    /// Environment variable holding the path of the credentials JSON (`username`, `password`).
    pub(crate) credentials_env: String,
    /// Environment variable holding the path of the pinned `known_hosts`.
    pub(crate) known_hosts_env: String,
    /// The ssh program (looked up on PATH unless it contains a slash).
    pub(crate) ssh: String,
    /// ssh `HostKeyAlgorithms` (the AP9538 NMC2 offers only `ssh-rsa`: `+ssh-rsa`).
    pub(crate) host_key_algorithms: Option<String>,
}

/// One switchable feed.
#[derive(Debug, Clone)]
pub(crate) struct Target {
    pub(crate) name: String,
    pub(crate) backend: Backend,
    /// What this feed powers; `off`/`cycle` print it before acting.
    pub(crate) feeds: Option<String>,
    pub(crate) cycle_off: Duration,
}

/// A device with one or more feeds (a switch with redundant PSUs).
#[derive(Debug, Clone)]
pub(crate) struct Device {
    pub(crate) name: String,
    pub(crate) description: String,
    pub(crate) feeds: Vec<String>,
}

/// The whole table.
#[derive(Debug)]
pub(crate) struct Config {
    pub(crate) targets: BTreeMap<String, Target>,
    pub(crate) devices: BTreeMap<String, Device>,
}

/// What a name on the command line refers to.
pub(crate) enum Named<'a> {
    Target(&'a Target),
    Device(&'a Device),
}

impl Config {
    /// The built-in table, or the file `LAB_POWER_CONFIG` names.
    pub(crate) fn load() -> Result<Self, String> {
        match std::env::var_os(CONFIG_ENV) {
            Some(path) => {
                let text = std::fs::read_to_string(&path)
                    .map_err(|e| format!("cannot read {CONFIG_ENV} file {}: {e}", path.display()))?;
                Self::parse(&text).map_err(|e| format!("{CONFIG_ENV} file {}: {e}", path.display()))
            }
            None => Self::parse(BUILTIN).map_err(|e| format!("built-in targets.json: {e}")),
        }
    }

    pub(crate) fn parse(text: &str) -> Result<Self, String> {
        let root: Value = serde_json::from_str(text).map_err(|e| format!("invalid JSON: {e}"))?;
        let root = object(&root, "the top level")?;
        only_keys(root, &["_comment", "targets", "devices"], "the top level")?;
        let mut targets = BTreeMap::new();
        for (name, spec) in object(root.get("targets").unwrap_or(&Value::Null), "targets")? {
            check_name(name)?;
            targets.insert(
                name.clone(),
                target(name, spec).map_err(|e| format!("target {name}: {e}"))?,
            );
        }
        if targets.is_empty() {
            return Err("no targets".to_owned());
        }
        let mut devices = BTreeMap::new();
        if let Some(spec) = root.get("devices") {
            for (name, spec) in object(spec, "devices")? {
                check_name(name)?;
                if targets.contains_key(name) {
                    return Err(format!("{name} is both a target and a device"));
                }
                let device = device(name, spec, &targets).map_err(|e| format!("device {name}: {e}"))?;
                devices.insert(name.clone(), device);
            }
        }
        Ok(Self { targets, devices })
    }

    pub(crate) fn lookup(&self, name: &str) -> Option<Named<'_>> {
        self.targets
            .get(name)
            .map(Named::Target)
            .or_else(|| self.devices.get(name).map(Named::Device))
    }

    pub(crate) fn names(&self) -> String {
        let mut names: Vec<&str> = self.targets.keys().map(String::as_str).collect();
        names.extend(self.devices.keys().map(String::as_str));
        names.join(", ")
    }
}

fn target(name: &str, spec: &Value) -> Result<Target, String> {
    let spec = object(spec, "the target")?;
    let backend = match string(spec, "backend")?.as_deref() {
        Some("home-assistant") => {
            only_keys(
                spec,
                &[
                    "backend",
                    "entity",
                    "power_sensor",
                    "feeds",
                    "cycle_off_seconds",
                    "note",
                ],
                "it",
            )?;
            let entity = string(spec, "entity")?.ok_or("needs entity")?;
            check_entity(&entity)?;
            let power_sensor = string(spec, "power_sensor")?;
            if let Some(sensor) = &power_sensor {
                check_entity(sensor)?;
            }
            Backend::HomeAssistant { entity, power_sensor }
        }
        Some("netbotz") => Backend::NetBotz(netbotz(spec)?),
        Some(other) => return Err(format!("unknown backend {other:?} (home-assistant or netbotz)")),
        None => return Err("needs a backend".to_owned()),
    };
    let feeds = string(spec, "feeds")?;
    let cycle_off = match spec.get("cycle_off_seconds") {
        None => DEFAULT_CYCLE_OFF,
        Some(v) => v
            .as_f64()
            .filter(|s| s.is_finite() && *s > 0.0 && *s <= 3600.0)
            .and_then(|s| Duration::try_from_secs_f64(s).ok())
            .ok_or("cycle_off_seconds must be a positive number up to 3600")?,
    };
    let _ = string(spec, "note")?;
    Ok(Target {
        name: name.to_owned(),
        backend,
        feeds,
        cycle_off,
    })
}

fn netbotz(spec: &Map<String, Value>) -> Result<NetBotz, String> {
    only_keys(
        spec,
        &[
            "backend",
            "host",
            "port",
            "credentials_env",
            "known_hosts_env",
            "ssh",
            "host_key_algorithms",
            "feeds",
            "cycle_off_seconds",
            "note",
        ],
        "it",
    )?;
    let host = string(spec, "host")?.ok_or("needs host")?;
    if host.is_empty() || host.starts_with('-') || !host.chars().all(|c| c.is_ascii_alphanumeric() || ".-:".contains(c))
    {
        return Err("host must be a hostname or IP address".to_owned());
    }
    let port = match spec.get("port") {
        None => 22,
        Some(v) => v
            .as_u64()
            .and_then(|p| u16::try_from(p).ok())
            .filter(|p| *p > 0)
            .ok_or("port must be 1-65535")?,
    };
    let credentials_env = string(spec, "credentials_env")?.ok_or("needs credentials_env")?;
    let known_hosts_env = string(spec, "known_hosts_env")?.ok_or("needs known_hosts_env")?;
    for var in [&credentials_env, &known_hosts_env] {
        if var.is_empty()
            || !var
                .chars()
                .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_')
        {
            return Err(format!("{var:?} is not an environment variable name"));
        }
    }
    let ssh = string(spec, "ssh")?.unwrap_or_else(|| "ssh".to_owned());
    if ssh.is_empty() || ssh.starts_with('-') {
        return Err("ssh must name a program".to_owned());
    }
    let host_key_algorithms = string(spec, "host_key_algorithms")?;
    if let Some(a) = &host_key_algorithms
        && (a.is_empty() || !a.chars().all(|c| c.is_ascii_alphanumeric() || "@.,+-".contains(c)))
    {
        return Err("host_key_algorithms must be an ssh algorithm list".to_owned());
    }
    Ok(NetBotz {
        host,
        port,
        credentials_env,
        known_hosts_env,
        ssh,
        host_key_algorithms,
    })
}

fn device(name: &str, spec: &Value, targets: &BTreeMap<String, Target>) -> Result<Device, String> {
    let spec = object(spec, "the device")?;
    only_keys(spec, &["description", "feeds", "note"], "it")?;
    let description = string(spec, "description")?.unwrap_or_else(|| name.to_owned());
    let list = spec
        .get("feeds")
        .and_then(Value::as_array)
        .ok_or("feeds must be a list of target names")?;
    let mut feeds = Vec::new();
    for feed in list {
        let feed = feed.as_str().ok_or("feeds must be a list of target names")?;
        if !targets.contains_key(feed) {
            return Err(format!("feed {feed:?} is not a target"));
        }
        if feeds.iter().any(|f| f == feed) {
            return Err(format!("feed {feed:?} is listed twice"));
        }
        feeds.push(feed.to_owned());
    }
    if feeds.is_empty() {
        return Err("needs at least one feed".to_owned());
    }
    Ok(Device {
        name: name.to_owned(),
        description,
        feeds,
    })
}

fn object<'a>(v: &'a Value, what: &str) -> Result<&'a Map<String, Value>, String> {
    v.as_object().ok_or_else(|| format!("{what} must be an object"))
}

fn only_keys(map: &Map<String, Value>, allowed: &[&str], what: &str) -> Result<(), String> {
    match map.keys().find(|k| !allowed.contains(&k.as_str())) {
        Some(key) => Err(format!("{what} has an unknown key {key:?}")),
        None => Ok(()),
    }
}

fn string(map: &Map<String, Value>, key: &str) -> Result<Option<String>, String> {
    match map.get(key) {
        None => Ok(None),
        Some(Value::String(s)) => Ok(Some(s.clone())),
        Some(_) => Err(format!("{key} must be a string")),
    }
}

fn check_name(name: &str) -> Result<(), String> {
    let ok = !name.is_empty()
        && !name.starts_with('-')
        && name.chars().all(|c| c.is_ascii_alphanumeric() || "._-".contains(c))
        && !ACTIONS.contains(&name);
    if ok {
        Ok(())
    } else {
        Err(format!("{name:?} is not a usable target name"))
    }
}

fn check_entity(entity: &str) -> Result<(), String> {
    let valid = |s: &str| {
        !s.is_empty()
            && s.chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
    };
    match entity.split_once('.') {
        Some((domain, object)) if valid(domain) && valid(object) => Ok(()),
        _ => Err(format!("{entity:?} is not a Home Assistant entity id")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtin_table_parses() {
        let c = Config::parse(BUILTIN).unwrap();
        assert!(matches!(c.lookup("1"), Some(Named::Target(_))));
        assert!(matches!(c.lookup("2"), Some(Named::Target(_))));
        let Some(Named::Target(nb)) = c.lookup("cs8210-psu1") else {
            panic!("netbotz target")
        };
        assert!(matches!(nb.backend, Backend::NetBotz(_)));
        let Some(Named::Device(dev)) = c.lookup("cs8210") else {
            panic!("device")
        };
        assert_eq!(dev.feeds, ["1", "cs8210-psu1"]);
        assert!(c.lookup("3").is_none());
    }

    #[test]
    fn bad_tables_refused() {
        for bad in [
            r#"{"targets": {}}"#,
            r#"{"targets": {"off": {"backend": "home-assistant", "entity": "switch.a"}}}"#,
            r#"{"targets": {"a": {"backend": "home-assistant", "entity": "switch.a", "extra": 1}}}"#,
            r#"{"targets": {"a": {"backend": "home-assistant", "entity": "Switch.A"}}}"#,
            r#"{"targets": {"a": {"backend": "x"}}}"#,
            r#"{"targets": {"a": {"backend": "netbotz", "host": "-oProxy", "credentials_env": "A", "known_hosts_env": "B"}}}"#,
            r#"{"targets": {"a": {"backend": "netbotz", "host": "h", "credentials_env": "a b", "known_hosts_env": "B"}}}"#,
            r#"{"targets": {"a": {"backend": "home-assistant", "entity": "switch.a"}}, "devices": {"d": {"feeds": ["b"]}}}"#,
            r#"{"targets": {"a": {"backend": "home-assistant", "entity": "switch.a"}}, "devices": {"a": {"feeds": ["a"]}}}"#,
            r#"{"targets": {"a": {"backend": "home-assistant", "entity": "switch.a"}}, "devices": {"d": {"feeds": ["a", "a"]}}}"#,
        ] {
            assert!(Config::parse(bad).is_err(), "{bad}");
        }
    }
}
