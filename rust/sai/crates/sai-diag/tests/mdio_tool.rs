//! The MDIO tool's `--verify-only --diag-socket` (what `phy-status` runs on the switches)
//! against this server on the virtual backend's MDIO model: the tool's expectations of the
//! answer format, and the hold-off path (`phy raw` through the core's foreign-MDIO hook).
#![allow(clippy::unwrap_used, clippy::panic, clippy::indexing_slicing, missing_docs)]

use sai_core::backend::Profile;
use sai_core::backend::types::{MdioDev, PhyAddr};
use sai_core::sai::Sai;
use sai_core::value::{Attr, Value};
use sai_core::virt::{ShellMdio, VirtualBackend};
use sai_diag::{DiagHandler, DiagHost, DiagServer, Level};
use std::ffi::{OsStr, OsString};
use std::os::fd::BorrowedFd;
use std::os::unix::fs::MetadataExt;
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};

struct Host {
    sai: Mutex<Sai<VirtualBackend>>,
    uid: u32,
}

impl DiagHost for Host {
    fn with_handler(&self, body: &mut dyn FnMut(Option<&mut dyn DiagHandler>)) {
        let mut sai = self.sai.lock().unwrap();
        body(Some(&mut *sai));
    }
    fn peer_uid(&self, _conn: BorrowedFd<'_>) -> Option<u32> {
        Some(self.uid)
    }
    fn log(&self, _level: Level, _text: &str) {}
}

const SCRIPT: &str = "mdio-script 1\nL init\nW 0 1 8201 7e\n\
    L verify phy 0x10\nP 10 1 8b00 2391 ffff 1 0\nP 10 1 8b01 80a0 ffff 1 0\nP 10 1 8251 000d 000d 1 0\n\
    L verify phy 0x11\nP 11 1 8b00 2391 ffff 1 0\n";
const NAMES: &str = "phy 10 cage1\nphy 11 cage2\nreg 1.8b00 id\n";

fn tool(args: &[&OsStr]) -> (u8, String, String) {
    let args: Vec<OsString> = args.iter().map(|a| (*a).to_owned()).collect();
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let never = AtomicBool::new(false);
    let mut env = mdio_tool::Env {
        out: &mut out,
        err: &mut err,
        stop: &never,
        arm_signals: &mut || {},
        opener: None,
        notify_socket: None,
        lock_dir: PathBuf::from("/nonexistent-lock-dir"),
    };
    let code = mdio_tool::run(OsStr::new("openbcm-mdio-script"), &args, &mut env);
    (code, String::from_utf8(out).unwrap(), String::from_utf8(err).unwrap())
}

#[test]
fn verify_only_through_the_diag_socket_on_the_virtual_chip() {
    let base = std::env::var_os("SAI_RS_TEST_TMPDIR").map_or_else(std::env::temp_dir, PathBuf::from);
    let dir = base.join(format!("sai-diag-mdio-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let script = dir.join("phys.mdio");
    std::fs::write(&script, SCRIPT).unwrap();
    let names = dir.join("names");
    std::fs::write(&names, NAMES).unwrap();

    let mut sai = Sai::new(VirtualBackend::new());
    sai.create_switch(
        &[Attr::new(sai_sys::SAI_SWITCH_ATTR_INIT_SWITCH, Value::Bool(true))],
        &Profile::from_pairs([("SAI_RS_VIRTUAL_PORTS", "4")]),
    )
    .unwrap();
    sai.backend_mut().set_shell_mdio(ShellMdio::Answer);
    let mdio = sai.backend().mdio();
    {
        let d1 = MdioDev::new(1).unwrap();
        let mut m = mdio.lock().unwrap();
        for phy in [0x10, 0x11] {
            m.regs.insert((PhyAddr(phy), d1, 0x8b00), 0x2391);
            m.regs.insert((PhyAddr(phy), d1, 0x8b01), 0x80a0);
            m.regs.insert((PhyAddr(phy), d1, 0x8251), 0x002d);
        }
    }
    let host = Arc::new(Host {
        sai: Mutex::new(sai),
        uid: std::fs::metadata(&dir).unwrap().uid(),
    });
    let sock = dir.join("diag.sock");
    let as_dyn: Arc<dyn DiagHost> = host.clone();
    let server = DiagServer::start(sock.to_str().unwrap(), as_dyn).unwrap();
    let args = [
        OsStr::new("--verify-only"),
        OsStr::new("--diag-socket"),
        sock.as_os_str(),
        OsStr::new("--names"),
        names.as_os_str(),
        script.as_os_str(),
    ];
    let (code, out, err) = tool(&args);
    assert_eq!((code, err.as_str()), (0, ""), "{out}");
    assert!(out.contains("2 of 2 PHYs as the script expects\n"), "{out}");
    assert!(out.contains("cage1  0x010  2391"), "{out}");
    // reads only, each one MDIO access of the model, each one shell command
    let log = mdio.lock().unwrap().log.clone();
    assert_eq!(log.len(), 4);
    assert!(log.iter().all(|op| !op.write));
    let shell: Vec<String> = host.sai.lock().unwrap().backend().shell_log().to_vec();
    assert_eq!(shell[0], "phy raw c45 0x10 0x1 0x8b00");
    assert!(shell.iter().all(|c| c.starts_with("phy raw c45 0x")));

    // a PHY that does not answer: the tool reports it from the server's failure line
    mdio.lock().unwrap().dead.insert(PhyAddr(0x11));
    let (code, out, err) = tool(&args);
    assert_eq!(code, 2, "{out}");
    assert_eq!(err, "openbcm-mdio-script: openbcm-diag: command failed (-1)\n");
    assert!(
        out.contains("1 of 2 PHYs are not as the script expects") || out.contains("of 2 PHYs"),
        "{out}"
    );

    // the C++ fake's echo is not an answer the tool accepts
    host.sai.lock().unwrap().backend_mut().set_shell_mdio(ShellMdio::Echo);
    let (code, _out, err) = tool(&args);
    assert_ne!(code, 0);
    assert!(err.contains("no value in the diag shell's answer"), "{err}");
    drop(server);
    std::fs::remove_dir_all(&dir).unwrap();
}
