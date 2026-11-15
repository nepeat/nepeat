//! A fake sysfs tree in a temporary directory, for tests and simulation.
//! Mirrors the real layout: /sys/class/hwmon/hwmonN are symlinks into
//! /sys/devices/..., each with a `device` link and optional `of_node`.

use std::fs;
use std::os::unix::fs::symlink;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};

static COUNTER: AtomicU32 = AtomicU32::new(0);

pub struct FakeSys {
    pub root: PathBuf,
}

impl FakeSys {
    pub fn new(tag: &str) -> FakeSys {
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!("fand-test-{tag}-{}-{n}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("sys/class/hwmon")).expect("create fake sysfs");
        FakeSys { root }
    }

    /// Write a file relative to the root (parents created), return its path.
    pub fn write(&self, rel: &str, content: &str) -> PathBuf {
        let p = self.root.join(rel.trim_start_matches('/'));
        if let Some(d) = p.parent() {
            fs::create_dir_all(d).expect("mkdir");
        }
        fs::write(&p, format!("{content}\n")).expect("write fake attr");
        p
    }

    /// Create hwmon<index> named `name` under /sys/devices/<device>, with an
    /// optional devicetree node /sys/firmware/devicetree/base/<of_node>.
    pub fn hwmon(&self, index: u32, name: &str, device: &str, of_node: Option<&str>) -> PathBuf {
        let dev = self.root.join("sys/devices").join(device);
        let dir = dev.join(format!("hwmon/hwmon{index}"));
        fs::create_dir_all(&dir).expect("mkdir hwmon");
        fs::write(dir.join("name"), format!("{name}\n")).expect("name");
        symlink(&dev, dir.join("device")).expect("device link");
        if let Some(node) = of_node {
            let n = self.root.join("sys/firmware/devicetree/base").join(node);
            fs::create_dir_all(&n).expect("of_node");
            symlink(&n, dev.join("of_node")).expect("of_node link");
        }
        symlink(&dir, self.root.join(format!("sys/class/hwmon/hwmon{index}"))).expect("class link");
        self.root.join(format!("sys/class/hwmon/hwmon{index}"))
    }

    /// Set an attribute in a hwmon (or any) directory.
    pub fn set(&self, dir: &Path, attr: &str, value: &str) {
        fs::write(dir.join(attr), format!("{value}\n")).expect("set attr");
    }

    pub fn get(&self, dir: &Path, attr: &str) -> String {
        fs::read_to_string(dir.join(attr))
            .unwrap_or_default()
            .trim()
            .to_string()
    }

    pub fn remove(&self, dir: &Path, attr: &str) {
        let _ = fs::remove_file(dir.join(attr));
    }
}

impl Drop for FakeSys {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}
