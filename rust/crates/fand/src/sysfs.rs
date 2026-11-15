//! sysfs access with an injectable root, and robust hwmon resolution.
//!
//! hwmonN numbers depend on probe order, so devices are found by driver
//! `name`, a substring of the resolved `device` path, or a substring of the
//! resolved `device/of_node` path (devicetree boards). Everything here is
//! read-only except [`write_value`].

use crate::config::HwmonSelector;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

/// Map an absolute device path into the (possibly fake) root.
pub fn rebase(root: &Path, p: &Path) -> PathBuf {
    if root == Path::new("/") || root.as_os_str().is_empty() {
        return p.to_path_buf();
    }
    root.join(p.strip_prefix("/").unwrap_or(p))
}

pub fn read_trim(p: &Path) -> io::Result<String> {
    Ok(fs::read_to_string(p)?.trim().to_string())
}

/// Parse a sysfs-style number (integer or decimal).
pub fn parse_number(s: &str) -> Result<f64, String> {
    let t = s.trim();
    let tok = t.split_whitespace().next().unwrap_or("");
    let v: f64 = tok
        .parse()
        .map_err(|_| format!("not a number: {:?}", truncate(t, 40)))?;
    if !v.is_finite() {
        return Err(format!("not a finite number: {tok:?}"));
    }
    Ok(v)
}

pub fn truncate(s: &str, n: usize) -> String {
    if s.chars().count() <= n {
        s.to_string()
    } else {
        let mut out: String = s.chars().take(n).collect();
        out.push_str("...");
        out
    }
}

/// Read a number; errors carry the path. NotFound is reported separately so
/// callers can re-resolve devices that were re-probed.
pub fn read_number(p: &Path) -> Result<f64, ReadError> {
    match fs::read_to_string(p) {
        Ok(s) => parse_number(&s).map_err(|e| ReadError::Other(format!("{}: {e}", p.display()))),
        Err(e) if e.kind() == io::ErrorKind::NotFound => ReadError::not_found(p),
        Err(e) => Err(ReadError::Other(format!("{}: {e}", p.display()))),
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum ReadError {
    NotFound(String),
    Other(String),
}

impl ReadError {
    fn not_found<T>(p: &Path) -> Result<T, ReadError> {
        Err(ReadError::NotFound(format!("{}: no such file", p.display())))
    }
    pub fn message(self) -> String {
        match self {
            ReadError::NotFound(s) | ReadError::Other(s) => s,
        }
    }
}

/// Write a value to an existing attribute (never creates files).
pub fn write_value(p: &Path, v: &str) -> io::Result<()> {
    let mut f = fs::OpenOptions::new().write(true).truncate(true).open(p)?;
    f.write_all(format!("{v}\n").as_bytes())?;
    f.flush()
}

#[derive(Debug, Clone)]
pub struct HwmonDev {
    pub dir: PathBuf,
    pub name: String,
    pub device: Option<String>,
    pub of_node: Option<String>,
}

fn canon(p: &Path) -> Option<String> {
    fs::canonicalize(p).ok().map(|p| p.to_string_lossy().into_owned())
}

/// All hwmon devices under <root>/sys/class/hwmon, sorted by device path.
pub fn list_hwmon(root: &Path) -> io::Result<Vec<HwmonDev>> {
    let class = rebase(root, Path::new("/sys/class/hwmon"));
    let mut out = Vec::new();
    for entry in fs::read_dir(&class)? {
        let dir = entry?.path();
        let name = read_trim(&dir.join("name"))
            .or_else(|_| read_trim(&dir.join("device/name")))
            .unwrap_or_default();
        out.push(HwmonDev {
            device: canon(&dir.join("device")),
            of_node: canon(&dir.join("device/of_node")),
            name,
            dir,
        });
    }
    out.sort_by(|a, b| (&a.device, &a.dir).cmp(&(&b.device, &b.dir)));
    Ok(out)
}

fn matches(d: &HwmonDev, sel: &HwmonSelector) -> bool {
    sel.name.as_ref().is_none_or(|n| &d.name == n)
        && sel
            .device
            .as_ref()
            .is_none_or(|s| d.device.as_ref().is_some_and(|p| p.contains(s.as_str())))
        && sel
            .of_node
            .as_ref()
            .is_none_or(|s| d.of_node.as_ref().is_some_and(|p| p.contains(s.as_str())))
}

/// Resolve a selector to a hwmon directory. Ambiguity is an error unless an
/// index is given, so a second matching chip can never be silently picked.
pub fn find_hwmon(root: &Path, sel: &HwmonSelector) -> Result<PathBuf, String> {
    if let Some(p) = &sel.path {
        let p = rebase(root, p);
        if p.join("name").exists() {
            return Ok(p);
        }
        // A device directory with a single hwmon child.
        let hw = p.join("hwmon");
        let kids: Vec<PathBuf> = fs::read_dir(&hw)
            .map(|r| r.filter_map(|e| e.ok().map(|e| e.path())).collect())
            .unwrap_or_default();
        return match kids.as_slice() {
            [one] => Ok(one.clone()),
            _ => Err(format!("hwmon {sel}: {} is not a hwmon directory", p.display())),
        };
    }
    let all = list_hwmon(root).map_err(|e| format!("listing hwmon devices: {e}"))?;
    let found: Vec<&HwmonDev> = all.iter().filter(|d| matches(d, sel)).collect();
    match (found.len(), sel.index) {
        (0, _) => Err(format!(
            "no hwmon device matches {sel} (present: {})",
            if all.is_empty() {
                "none".to_string()
            } else {
                all.iter().map(|d| d.name.as_str()).collect::<Vec<_>>().join(", ")
            }
        )),
        (n, Some(i)) => found
            .get(i)
            .map(|d| d.dir.clone())
            .ok_or_else(|| format!("hwmon {sel}: only {n} devices match")),
        (1, None) => Ok(found[0].dir.clone()),
        (n, None) => Err(format!(
            "{n} hwmon devices match {sel}: {}; add device/of_node (or index) to the selector",
            found
                .iter()
                .map(|d| d.device.clone().unwrap_or_else(|| d.dir.display().to_string()))
                .collect::<Vec<_>>()
                .join(", ")
        )),
    }
}

/// Attribute lookup: the hwmon directory first, then its device directory
/// (older drivers put attributes there).
pub fn attr(dir: &Path, name: &str) -> Option<PathBuf> {
    [dir.join(name), dir.join("device").join(name)]
        .into_iter()
        .find(|p| p.exists())
}

fn numbered(dir: &Path, prefix: &str, suffix: &str) -> Vec<(u32, String)> {
    let mut out = Vec::new();
    for d in [dir.to_path_buf(), dir.join("device")] {
        let Ok(rd) = fs::read_dir(&d) else { continue };
        for e in rd.flatten() {
            let n = e.file_name().to_string_lossy().into_owned();
            if let Some(num) = n
                .strip_prefix(prefix)
                .and_then(|r| r.strip_suffix(suffix))
                .and_then(|r| r.parse::<u32>().ok())
            {
                if !out.iter().any(|(k, _)| *k == num) {
                    out.push((num, n));
                }
            }
        }
    }
    out.sort();
    out
}

/// "temp1" / "temp1_input" / label lookup within a hwmon directory.
pub fn find_temp_input(dir: &Path, input: Option<&str>, label: Option<&str>) -> Result<PathBuf, String> {
    if let Some(l) = label {
        for (n, _) in numbered(dir, "temp", "_label") {
            if let Some(p) = attr(dir, &format!("temp{n}_label")) {
                if read_trim(&p).is_ok_and(|s| s == l) {
                    return attr(dir, &format!("temp{n}_input")).ok_or_else(|| {
                        format!("{}: temp{n}_label is {l:?} but temp{n}_input is missing", dir.display())
                    });
                }
            }
        }
        let labels: Vec<String> = numbered(dir, "temp", "_label")
            .iter()
            .filter_map(|(n, _)| attr(dir, &format!("temp{n}_label")).and_then(|p| read_trim(&p).ok()))
            .collect();
        return Err(format!(
            "{}: no temperature labelled {l:?} (labels: {})",
            dir.display(),
            if labels.is_empty() {
                "none".into()
            } else {
                labels.join(", ")
            }
        ));
    }
    let name = normalize_input(input.unwrap_or("temp1"));
    attr(dir, &name).ok_or_else(|| format!("{}: no attribute {name}", dir.display()))
}

/// "fan1" -> "fan1_input"; anything with an underscore is kept.
pub fn normalize_input(s: &str) -> String {
    if s.contains('_') {
        s.to_string()
    } else {
        format!("{s}_input")
    }
}

/// fanN_input attributes of a hwmon directory, as ("fanN", path), numeric order.
pub fn list_fans(dir: &Path) -> Vec<(String, PathBuf)> {
    numbered(dir, "fan", "_input")
        .into_iter()
        .filter_map(|(n, file)| attr(dir, &file).map(|p| (format!("fan{n}"), p)))
        .collect()
}

/// Thermal zone by type, directory name or number -> its "temp" file.
pub fn find_thermal_zone(root: &Path, zone: &str) -> Result<PathBuf, String> {
    let class = rebase(root, Path::new("/sys/class/thermal"));
    let direct = if zone.chars().all(|c| c.is_ascii_digit()) {
        format!("thermal_zone{zone}")
    } else {
        zone.to_string()
    };
    let d = class.join(&direct);
    if direct.starts_with("thermal_zone") && d.join("temp").exists() {
        return Ok(d.join("temp"));
    }
    let mut found = Vec::new();
    if let Ok(rd) = fs::read_dir(&class) {
        for e in rd.flatten() {
            let p = e.path();
            if e.file_name().to_string_lossy().starts_with("thermal_zone")
                && read_trim(&p.join("type")).is_ok_and(|t| t == zone)
            {
                found.push(p);
            }
        }
    }
    found.sort();
    match found.as_slice() {
        [one] => Ok(one.join("temp")),
        [] => Err(format!("no thermal zone of type {zone:?} under {}", class.display())),
        many => Err(format!(
            "{} thermal zones have type {zone:?}; use thermal_zoneN",
            many.len()
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fakesys::FakeSys;

    #[test]
    fn resolves_by_name_device_and_of_node() {
        let fs = FakeSys::new("resolve");
        fs.hwmon(
            0,
            "lm75",
            "platform/soc/i2c-3/3-0049",
            Some("soc/i2c@119000/mux@76/i2c@1/temperature-sensor@49"),
        );
        fs.hwmon(1, "as7760_fan", "platform/soc/i2c-2/2-0066", None);
        fs.hwmon(
            2,
            "lm75",
            "platform/soc/i2c-3/3-0048",
            Some("soc/i2c@119000/mux@76/i2c@1/temperature-sensor@48"),
        );
        let sel = |n: Option<&str>, d: Option<&str>, o: Option<&str>| HwmonSelector {
            name: n.map(String::from),
            device: d.map(String::from),
            of_node: o.map(String::from),
            ..Default::default()
        };
        let fan = find_hwmon(&fs.root, &sel(Some("as7760_fan"), None, None)).unwrap();
        assert!(fan.ends_with("hwmon1"));
        let err = find_hwmon(&fs.root, &sel(Some("lm75"), None, None)).unwrap_err();
        assert!(err.contains("2 hwmon devices match"), "{err}");
        let by_of = find_hwmon(&fs.root, &sel(Some("lm75"), None, Some("temperature-sensor@48"))).unwrap();
        assert!(by_of.ends_with("hwmon2"));
        let by_dev = find_hwmon(&fs.root, &sel(None, Some("3-0049"), None)).unwrap();
        assert!(by_dev.ends_with("hwmon0"));
        // index picks among matches sorted by device path: 3-0048 first.
        let idx = find_hwmon(
            &fs.root,
            &HwmonSelector {
                index: Some(0),
                ..sel(Some("lm75"), None, None)
            },
        )
        .unwrap();
        assert!(idx.ends_with("hwmon2"));
        assert!(
            find_hwmon(&fs.root, &sel(Some("nct6775"), None, None))
                .unwrap_err()
                .contains("present: ")
        );
        let by_path = find_hwmon(
            &fs.root,
            &HwmonSelector {
                path: Some("/sys/class/hwmon/hwmon1".into()),
                ..Default::default()
            },
        )
        .unwrap();
        assert!(by_path.ends_with("hwmon1"));
    }

    #[test]
    fn finds_inputs_by_label_and_fans_in_order() {
        let fs = FakeSys::new("labels");
        let d = fs.hwmon(0, "coretemp", "platform/coretemp.0", None);
        fs.set(&d, "temp1_input", "45000");
        fs.set(&d, "temp1_label", "Package id 0");
        fs.set(&d, "temp2_input", "41000");
        fs.set(&d, "temp2_label", "Core 0");
        for n in [10, 2, 1] {
            fs.set(&d, &format!("fan{n}_input"), "1000");
        }
        assert!(
            find_temp_input(&d, None, Some("Core 0"))
                .unwrap()
                .ends_with("temp2_input")
        );
        assert!(
            find_temp_input(&d, Some("temp1"), None)
                .unwrap()
                .ends_with("temp1_input")
        );
        assert!(
            find_temp_input(&d, None, Some("Core 9"))
                .unwrap_err()
                .contains("Package id 0, Core 0")
        );
        let fans: Vec<String> = list_fans(&d).into_iter().map(|f| f.0).collect();
        assert_eq!(fans, ["fan1", "fan2", "fan10"]);
    }

    #[test]
    fn thermal_zone_lookup() {
        let fs = FakeSys::new("thermal");
        fs.write("sys/class/thermal/thermal_zone0/type", "acpitz");
        fs.write("sys/class/thermal/thermal_zone0/temp", "27800");
        fs.write("sys/class/thermal/thermal_zone1/type", "x86_pkg_temp");
        fs.write("sys/class/thermal/thermal_zone1/temp", "51000");
        assert!(
            find_thermal_zone(&fs.root, "x86_pkg_temp")
                .unwrap()
                .ends_with("thermal_zone1/temp")
        );
        assert!(
            find_thermal_zone(&fs.root, "0")
                .unwrap()
                .ends_with("thermal_zone0/temp")
        );
        assert!(
            find_thermal_zone(&fs.root, "thermal_zone1")
                .unwrap()
                .ends_with("thermal_zone1/temp")
        );
        assert!(find_thermal_zone(&fs.root, "nope").is_err());
    }

    #[test]
    fn numbers_and_writes() {
        assert_eq!(parse_number("42000\n").unwrap(), 42000.0);
        assert_eq!(parse_number(" 41.5 C").unwrap(), 41.5);
        assert!(parse_number("").is_err());
        assert!(parse_number("NaN").is_err());
        let fs = FakeSys::new("write");
        let p = fs.write("x/pwm1", "255");
        write_value(&p, "51").unwrap();
        assert_eq!(read_trim(&p).unwrap(), "51");
        assert!(
            write_value(&fs.root.join("x/missing"), "1").is_err(),
            "must not create files"
        );
        assert!(matches!(
            read_number(&fs.root.join("x/missing")),
            Err(ReadError::NotFound(_))
        ));
    }
}
