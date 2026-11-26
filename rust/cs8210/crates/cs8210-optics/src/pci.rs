//! The I2C FPGA on PCI, from sysfs: finding it, its command register (memory decode),
//! BAR0, and the root port's view of the link.
//!
//! A dead FPGA (2026-11-16) turns every BAR or config access into a completion timeout
//! that stalls the whole `SoC` for seconds. The upstream root port's config space is on
//! the `SoC` and never stalls: data link down (DLLLA clear where the port reports it),
//! `DevSta` NonFatal/Fatal, or AER `UESta` `CmpltTO` there means the FPGA is gone.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::regs::{DEVICE, MAP_LEN, VENDOR};

const EXP_DEVSTA: usize = 0x0a;
const EXP_LNKCAP: usize = 0x0c;
const EXP_LNKSTA: usize = 0x12;
const DEVSTA_NONFATAL_FATAL: u16 = 0x6;
const LNKCAP_DLLLA: u32 = 1 << 20;
const LNKSTA_DLLLA: u16 = 1 << 13;
const AER_UESTA: usize = 0x04;
const UE_CMPLT_TO: u32 = 1 << 14;
const COMMAND_MEMORY: u16 = 0x2;

fn le16(c: &[u8], o: usize) -> Option<u16> {
    Some(u16::from_le_bytes([*c.get(o)?, *c.get(o + 1)?]))
}

fn le32(c: &[u8], o: usize) -> Option<u32> {
    Some(u32::from_le_bytes([
        *c.get(o)?,
        *c.get(o + 1)?,
        *c.get(o + 2)?,
        *c.get(o + 3)?,
    ]))
}

/// (`PCIe` capability offset, AER extended capability offset) in a config space image.
#[must_use]
pub fn pcie_caps(cfg: &[u8]) -> (Option<usize>, Option<usize>) {
    let mut exp = None;
    if cfg.len() >= 0x40 && cfg.get(6).is_some_and(|s| s & 0x10 != 0) {
        let mut p = usize::from(cfg.get(0x34).copied().unwrap_or(0) & 0xfc);
        let mut seen = Vec::new();
        while p != 0 && !seen.contains(&p) && p + 2 <= cfg.len() {
            seen.push(p);
            if cfg.get(p) == Some(&0x10) {
                exp = Some(p);
                break;
            }
            p = usize::from(cfg.get(p + 1).copied().unwrap_or(0) & 0xfc);
        }
    }
    let mut aer = None;
    let mut p = 0x100;
    let mut seen = Vec::new();
    while p >= 0x100 && p + 4 <= cfg.len() && !seen.contains(&p) {
        seen.push(p);
        let Some(h) = le32(cfg, p) else { break };
        if h == 0 || h == 0xffff_ffff {
            break;
        }
        if h & 0xffff == 0x0001 {
            aer = Some(p);
            break;
        }
        p = usize::try_from((h >> 20) & 0xffc).unwrap_or(0);
    }
    (exp, aer)
}

/// Why the root port says the device behind it is gone, or None.
#[must_use]
pub fn upstream_problem(cfg: &[u8], exp: Option<usize>, aer: Option<usize>) -> Option<String> {
    let mut why = Vec::new();
    if let Some(e) = exp {
        let devsta = le16(cfg, e + EXP_DEVSTA).unwrap_or(0);
        if devsta & DEVSTA_NONFATAL_FATAL != 0 {
            why.push(format!("DevSta {devsta:#06x} (non-fatal/fatal error)"));
        }
        let lnksta = le16(cfg, e + EXP_LNKSTA).unwrap_or(0);
        if le32(cfg, e + EXP_LNKCAP).unwrap_or(0) & LNKCAP_DLLLA != 0 && lnksta & LNKSTA_DLLLA == 0 {
            why.push(format!("LnkSta {lnksta:#06x} (data link down)"));
        }
    }
    if let Some(a) = aer {
        let ue = le32(cfg, a + AER_UESTA).unwrap_or(0);
        if ue & UE_CMPLT_TO != 0 {
            why.push(format!("AER UESta {ue:#010x} (completion timeout)"));
        }
    }
    (!why.is_empty()).then(|| why.join("; "))
}

/// The FPGA's sysfs device.
#[derive(Debug, Clone)]
pub struct Device {
    /// bus/device/function
    pub bdf: String,
    dir: PathBuf,
    root_port: Option<(PathBuf, Option<usize>, Option<usize>)>,
}

fn read_hex(p: &Path) -> Option<u32> {
    let t = fs::read_to_string(p).ok()?;
    u32::from_str_radix(t.trim().trim_start_matches("0x"), 16).ok()
}

impl Device {
    /// The one `1d0f:8571` under `pci_root` (`/sys/bus/pci/devices`).
    pub fn find(pci_root: &Path) -> Result<Self, String> {
        let mut hits = Vec::new();
        let rd = fs::read_dir(pci_root).map_err(|e| format!("{}: {e}", pci_root.display()))?;
        for e in rd.flatten() {
            let d = e.path();
            if read_hex(&d.join("vendor")) == Some(u32::from(VENDOR))
                && read_hex(&d.join("device")) == Some(u32::from(DEVICE))
            {
                hits.push(d);
            }
        }
        hits.sort();
        match hits.as_slice() {
            [d] => {
                let bdf = d
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default();
                Ok(Self {
                    bdf,
                    dir: d.clone(),
                    root_port: None,
                })
            }
            _ => Err(format!(
                "{} PCI devices {VENDOR:04x}:{DEVICE:04x} (want exactly one)",
                hits.len()
            )),
        }
    }

    /// The BAR0 resource file.
    #[must_use]
    pub fn resource0(&self) -> PathBuf {
        self.dir.join("resource0")
    }

    /// PCI command register.
    pub fn command(&self) -> io::Result<u16> {
        let c = fs::read(self.dir.join("config"))?;
        le16(&c, 4).ok_or_else(|| io::Error::other("short config space"))
    }

    /// Memory decode on.
    pub fn decode_on(&self) -> io::Result<bool> {
        Ok(self.command()? & COMMAND_MEMORY != 0)
    }

    /// What the vendor probe's `pci_enable_device` does: `echo 1 > enable`.
    pub fn enable(&self) -> io::Result<()> {
        fs::write(self.dir.join("enable"), "1")
    }

    /// A kernel driver bound to it.
    #[must_use]
    pub fn driver(&self) -> Option<String> {
        fs::read_link(self.dir.join("driver"))
            .ok()
            .and_then(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()))
    }

    /// Checks before mapping: no driver, BAR0 assigned and long enough.
    pub fn check_bar(&self) -> Result<(), String> {
        if let Some(d) = self.driver() {
            return Err(format!("{} is bound to kernel driver {d}", self.bdf));
        }
        let t = fs::read_to_string(self.dir.join("resource")).map_err(|e| format!("{}: resource: {e}", self.bdf))?;
        let mut it = t
            .lines()
            .next()
            .unwrap_or("")
            .split_whitespace()
            .map(|x| u64::from_str_radix(x.trim_start_matches("0x"), 16));
        let (Some(Ok(start)), Some(Ok(end))) = (it.next(), it.next()) else {
            return Err(format!("{}: cannot parse resource", self.bdf));
        };
        if start == 0 || end < start || end - start + 1 < MAP_LEN as u64 {
            return Err(format!("{}: BAR0 unassigned or shorter than {MAP_LEN:#x}", self.bdf));
        }
        Ok(())
    }

    /// The root port's complaint about the link, if any (None also when there is no
    /// root port status to read: an unknown topology).
    pub fn upstream_problem(&mut self) -> Option<String> {
        if self.root_port.is_none() {
            let real = fs::canonicalize(&self.dir).ok()?;
            let cfg = real.parent()?.join("config");
            let (exp, aer) = fs::read(&cfg).map_or((None, None), |c| pcie_caps(&c));
            self.root_port = Some((cfg, exp, aer));
        }
        let (cfg, exp, aer) = self.root_port.clone()?;
        if exp.is_none() && aer.is_none() {
            return None;
        }
        let c = fs::read(&cfg).ok()?;
        upstream_problem(&c, exp, aer)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The I2C FPGA's root port (0002:00:00.0) as read live on 2026-11-16 (`fake_board.py`).
    fn root_port() -> Vec<u8> {
        let mut c = vec![0u8; 4096];
        c[6] = 0x10;
        c[0x34] = 0x40;
        c[0x40..0x42].copy_from_slice(&[0x01, 0x50]);
        c[0x50..0x52].copy_from_slice(&[0x05, 0x70]);
        c[0x70..0x72].copy_from_slice(&[0x10, 0x00]);
        c[0x7c..0x80].copy_from_slice(&0x0073_6c12u32.to_le_bytes());
        c[0x82..0x84].copy_from_slice(&0x3012u16.to_le_bytes());
        c[0x100..0x104].copy_from_slice(&(0x0001u32 | 2 << 16 | 0x148 << 20).to_le_bytes());
        c
    }

    #[test]
    fn healthy_and_sick_root_port() {
        let mut c = root_port();
        let (exp, aer) = pcie_caps(&c);
        assert_eq!((exp, aer), (Some(0x70), Some(0x100)));
        assert_eq!(upstream_problem(&c, exp, aer), None);
        c[0x104..0x108].copy_from_slice(&UE_CMPLT_TO.to_le_bytes());
        assert!(upstream_problem(&c, exp, aer).unwrap().contains("completion timeout"));
        let mut c = root_port();
        c[0x82..0x84].copy_from_slice(&0x1012u16.to_le_bytes()); // DLLLA clear
        assert!(upstream_problem(&c, exp, aer).unwrap().contains("data link down"));
        let mut c = root_port();
        c[0x7a] = 0x04;
        assert!(upstream_problem(&c, exp, aer).unwrap().contains("DevSta"));
    }
}
