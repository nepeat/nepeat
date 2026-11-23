//! The hardware smoke program (`smoke/sai_smoke.c`, the G2 gate on the AS7772) on the fake
//! glue: its refusals and usage without a library; then the whole program against a `libsai.so`
//! built with sai-ffi's feature `openbcm-fake` (the openbcm backend over sdk-glue's Rust fake
//! chip, whose scenario is the profile's `SAI_INIT_CONFIG_FILE`), with and without an external
//! PHY table, and its failure paths.
//!
//! The library comes from `SAI_SMOKE_LIBSAI` (`nix/libsai-openbcm.nix` sets it; by hand:
//! `cargo build -p sai-ffi --features openbcm-fake`, then `target/debug/libsai.so`); without it
//! the runs skip with a note. The program: `$SAI_RS_TEST_BIN/sai-openbcm-smoke` (the test bundle)
//! or the one this crate's build script made.
//!
//! What only the switch can show (the fake has no KNET, no link scan thread, no PHYs): the kernel
//! netdev and its carrier (here --no-kernel, and one run proving that without it a missing netdev
//! is a FAIL), link through real PHYs (here the gate is driven by the fake's "fake linkscan"
//! diag command, --poke-diag), what the SDK prints.
#![allow(clippy::unwrap_used, clippy::panic, clippy::indexing_slicing, reason = "test code")]

mod common;

use common::{note, scenario};
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicUsize, Ordering};

fn program() -> Option<PathBuf> {
    if let Some(dir) = std::env::var_os("SAI_RS_TEST_BIN") {
        let exe = PathBuf::from(dir).join("sai-openbcm-smoke");
        if exe.is_file() {
            return Some(exe);
        }
    }
    let built = env!("SAI_OPENBCM_SMOKE_BIN");
    (!built.is_empty())
        .then(|| PathBuf::from(built))
        .filter(|p| p.is_file())
}

fn text(out: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
}

/// The program and the fake-glue library, or a note why the test skips.
fn setup() -> Option<(PathBuf, PathBuf)> {
    if let (Some(exe), Some(lib)) = (program(), std::env::var_os("SAI_SMOKE_LIBSAI")) {
        return Some((exe, PathBuf::from(lib)));
    }
    note("SKIPPED: needs the smoke program and SAI_SMOKE_LIBSAI=<libsai.so built with --features openbcm-fake>");
    None
}

/// A short directory of its own for a diag socket (unix socket paths have 107 bytes).
fn socket_dir() -> PathBuf {
    static N: AtomicUsize = AtomicUsize::new(0);
    let base = std::env::var_os("SAI_RS_TEST_TMPDIR").map_or_else(std::env::temp_dir, PathBuf::from);
    base.join(format!(
        "smk-{}-{}",
        std::process::id(),
        N.fetch_add(1, Ordering::Relaxed)
    ))
}

/// The AS7772's shape on the fake: 32 ports of 4 lanes at 100G (first lanes 1, 5, ..., 125),
/// two ASIC temperature monitors, the PHY id register the smoke reads, every glue call traced.
fn chip(trace: &Path, extra: &str) -> String {
    format!(
        "name BCM56960_B1\nports 32 4 100000\nlink admin\ntemp 450 470\nreg 0 1 8b00 2391\n{extra}trace {}\n",
        trace.display()
    )
}

/// `kExtPhyGood` of sai-openbcm's `tests/test_tables.h` without its hold-off (as sai-driver's
/// `10-ext-phy.ops`): port 1 gated by PHY 0x10 (with admin sequences), port 5 by PHY 0x11.
const EXT_PHY_TABLE: &str = "ext-phy 1\npoll-ms 0\nup-polls 2\nholdoff-ms 0\n\
    port 1\nlink\n  R 10 1 8221\n  W 10 1 a002 c161\n  W 10 1 8222 1111\n  P 10 1 8221 11e3 ffff 3 0\n  E 10 1 a100 2\n\
      C 10 1 a101 3 3\nadmin-down\n  W 10 1 d139 351\nadmin-up\n  E 10 1 d0e1 a008 ff7f\n  W 10 1 d139 350\n\
    port 5\nlink\n  C 11 1 a101 3 3\n";

/// The PHY registers that table reads, with its line side up.
const EXT_PHY_REGS: &str =
    "reg 10 1 8221 11e3\nreg 10 1 a100 2\nreg 10 1 a101 3\nreg 10 1 d0e1 a008\nreg 11 1 a101 3\n";

/// One fake switch: a profile as syncd gets it on the switch (the board quirks, a diag socket) on
/// a scenario, the trace file, and the run.
struct Fake {
    exe: PathBuf,
    lib: PathBuf,
    profile: PathBuf,
    trace: PathBuf,
}

impl Fake {
    fn new(name: &str, exe: &Path, lib: &Path, chip_extra: &str, profile_extra: &str) -> Self {
        let trace = common::scratch().join(format!("{name}-trace.txt"));
        let _ = std::fs::remove_file(&trace);
        let config = scenario(&format!("{name}.cfg"), &chip(&trace, chip_extra));
        let profile = scenario(
            &format!("{name}.profile"),
            &format!(
                "# as SONiC writes it\nSAI_OPENBCM_BACKEND=openbcm\nSAI_INIT_CONFIG_FILE={}\n\
                 SAI_OPENBCM_PORT_INTERFACE=caui\nSAI_OPENBCM_PORT_AUTONEG=off\nSAI_OPENBCM_PORT_FEC=none\n\
                 SAI_OPENBCM_PORT_IFG=128\nSAI_OPENBCM_DIAG_SOCKET={}\n{profile_extra}",
                config.display(),
                socket_dir().join("d.sock").display()
            ),
        );
        Self {
            exe: exe.to_owned(),
            lib: lib.to_owned(),
            profile,
            trace,
        }
    }

    fn run(&self, extra: &[&str]) -> (Option<i32>, String) {
        let out = Command::new(&self.exe)
            .arg("--sonic-is-stopped")
            .arg("--libsai")
            .arg(&self.lib)
            .arg("--profile")
            .arg(&self.profile)
            .args([
                "--cabled",
                "1,5,121,125",
                "--link-timeout",
                "5",
                "--mdio-expect",
                "0x2391",
            ])
            .args(extra)
            .output()
            .unwrap();
        let all = text(&out);
        println!("{all}");
        (out.status.code(), all)
    }

    fn trace(&self) -> String {
        std::fs::read_to_string(&self.trace).unwrap()
    }
}

/// The step's line ("PASS <step> ..."), or a panic showing the whole output.
fn line<'a>(all: &'a str, verdict: &str, step: &str) -> &'a str {
    let head = format!("{verdict} {step} ");
    all.lines()
        .find(|l| l.starts_with(&head) || *l == head.trim_end())
        .unwrap_or_else(|| panic!("no '{head}' line in:\n{all}"))
}

/// The MDIO writes in a trace ("W <phy> <dev> <reg> <val>").
fn mdio_writes(trace: &str) -> Vec<&str> {
    trace.lines().filter(|l| l.starts_with("W ")).collect()
}

#[test]
#[allow(clippy::too_many_lines, reason = "one invocation after another")]
fn the_smoke_program_refuses_and_lists_its_steps_without_a_library() {
    let Some(exe) = program() else {
        note("SKIPPED: the smoke program was not built (needs SAI_INC and a C compiler at build time)");
        return;
    };
    let profile = scenario("smoke-refuse.profile", "SAI_INIT_CONFIG_FILE=/nonexistent\n");
    let run = |args: &[&str]| Command::new(&exe).args(args).output().unwrap();
    // no confirmation: refused before the library is even loaded (the path does not exist)
    let p = profile.to_str().unwrap();
    let out = run(&["--libsai", "/nonexistent/libsai.so", "--profile", p, "--cabled", "1"]);
    assert_eq!(out.status.code(), Some(2), "{}", text(&out));
    assert!(
        text(&out).contains("refusing: creating the switch resets the switch chip"),
        "{}",
        text(&out)
    );
    // usage: no arguments (the bundle's probe), no library / profile / cabled ports, bad values
    for args in [
        &[][..],
        &["--sonic-is-stopped"],
        &["--sonic-is-stopped", "--libsai", "x", "--profile", p],
        &["--sonic-is-stopped", "--libsai", "x", "--profile", p, "--cabled", "1,1"],
        &[
            "--sonic-is-stopped",
            "--libsai",
            "x",
            "--profile",
            p,
            "--cabled",
            "1",
            "--vlan",
            "1",
        ],
        &[
            "--sonic-is-stopped",
            "--libsai",
            "x",
            "--profile",
            p,
            "--cabled",
            "1",
            "--mdio-phy",
            "0x800",
        ],
        &[
            "--sonic-is-stopped",
            "--libsai",
            "x",
            "--profile",
            p,
            "--cabled",
            "1",
            "--hostif-name",
            "a-much-too-long-name",
        ],
        &[
            "--sonic-is-stopped",
            "--libsai",
            "x",
            "--profile",
            p,
            "--cabled",
            "1",
            "--bogus",
        ],
    ] {
        let out = run(args);
        assert_eq!(out.status.code(), Some(2), "{args:?}: {}", text(&out));
        assert!(!text(&out).contains("PASS"), "{args:?}: {}", text(&out));
    }
    assert!(
        text(&run(&[
            "--sonic-is-stopped",
            "--libsai",
            "x",
            "--profile",
            p,
            "--cabled",
            "1",
            "--vlan",
            "1"
        ]))
        .contains("--vlan: a number 2..4094")
    );
    // --dry: the numbered steps, nothing loaded (no library, no profile, no confirmation needed)
    let out = run(&["--dry"]);
    let all = text(&out);
    assert_eq!(out.status.code(), Some(0), "{all}");
    let steps: Vec<&str> = all
        .lines()
        .filter_map(|l| l.split_once(". ").filter(|(n, _)| n.trim().parse::<u32>().is_ok()))
        .map(|(_, rest)| rest.split_whitespace().next().unwrap())
        .collect();
    assert_eq!(
        steps,
        [
            "load",
            "profile",
            "create-switch",
            "ports",
            "admin-up",
            "oper-up",
            "vlan",
            "fdb",
            "lag",
            "hostif",
            "hostif-netdev",
            "l3",
            "acl",
            "policer",
            "port-stats",
            "temperature",
            "diag",
            "mdio-read",
            "teardown",
            "admin-down",
            "remove-switch",
            "recreate",
            "uninitialize"
        ],
        "{all}"
    );
    assert!(
        all.contains("RESETS THE SWITCH CHIP") && all.contains("ONE clause-45 READ, device 1"),
        "{all}"
    );
}

#[test]
fn the_smoke_program_passes_on_the_fake_glue_and_a_second_switch_follows() {
    let Some((exe, lib)) = setup() else { return };
    let fake = Fake::new("smoke-plain", &exe, &lib, "", "");
    let (code, all) = fake.run(&["--no-kernel", "--recreate"]);
    assert_eq!(code, Some(0), "{all}");
    let summary = all
        .lines()
        .find(|l| l.starts_with("SMOKE_OK"))
        .unwrap_or_else(|| panic!("{all}"));
    note(&format!("smoke on the fake glue: {summary}"));
    assert!(!all.contains("\nFAIL "), "{all}");
    assert!(line(&all, "PASS", "ports").contains("ports=32") && line(&all, "PASS", "ports").contains("lanes=128"));
    assert!(
        line(&all, "PASS", "profile").contains("SAI_OPENBCM_EXT_PHY_FILE=(none)"),
        "{all}"
    );
    let oper = line(&all, "PASS", "oper-up");
    for lane in ["1", "5", "121", "125"] {
        assert!(oper.contains(&format!(" {lane}=up@")), "{oper}");
    }
    assert!(
        line(&all, "PASS", "temperature").contains("list=[45,47] max=47 average=46"),
        "{all}"
    );
    assert!(line(&all, "PASS", "diag").contains("first='fake: ps'"), "{all}");
    assert!(line(&all, "PASS", "mdio-read").contains("value=0x2391"), "{all}");
    assert!(line(&all, "SKIP", "hostif-netdev").contains("--no-kernel"), "{all}");
    assert!(line(&all, "PASS", "recreate").contains("ports=32 removed"), "{all}");
    assert!(all.contains("RISK recreate"), "{all}");
    // what reached the chip, in order, from the admin-up of the cabled ports (logical 1, 2, 31,
    // 32) to the end, without the per-port bring-up lines
    let chip = fake.trace();
    let from = chip.find("port 1 enable 1").unwrap_or_else(|| panic!("{chip}"));
    let second = chip.rfind("init-all").filter(|&e| e > from).unwrap_or(chip.len());
    let first = &chip[from..second];
    let wanted = [
        "port 1 enable 1",
        "vlan 100 create",
        "vlan 100 add 1 untagged",
        "l2 add 00:00:5e:00:53:01 100 0x1",
        "trunk create",
        "netif create sai-smoke0 port 1",
        "route add",
        "shell ps",
        "shell phy raw c45 0x0 0x1 0x8b00",
        "R 0 1 8b00 = 2391",
        "policer-destroy",
        "route delete",
        "netif destroy",
        "l2 delete 00:00:5e:00:53:01 100",
        "vlan 100 destroy",
        "port 1 enable 0",
        "shutdown unit 0",
    ];
    let mut at = 0;
    for w in wanted {
        let found = first[at..]
            .find(w)
            .unwrap_or_else(|| panic!("'{w}' not after offset {at} in:\n{first}"));
        at += found + w.len();
    }
    // the second switch: "init all" again, the ports disabled, shut down; no MDIO write anywhere
    assert_eq!(chip.matches("init-all unit 0").count(), 2, "{chip}");
    assert!(chip.ends_with("shutdown unit 0\n"), "{chip}");
    assert_eq!(mdio_writes(&chip), Vec::<&str>::new(), "{chip}");
}

#[test]
fn the_ext_phy_gate_runs_exactly_the_profiles_table_and_no_ext_phy_leaves_the_phys_alone() {
    let Some((exe, lib)) = setup() else { return };
    let table = scenario("smoke-ext-phy.tbl", EXT_PHY_TABLE);
    let fake = Fake::new(
        "smoke-ext",
        &exe,
        &lib,
        EXT_PHY_REGS,
        &format!("SAI_OPENBCM_EXT_PHY_FILE={}\n", table.display()),
    );
    // without the link scan pass the fake cannot bring a gated port up: oper-up fails for the
    // two gated ports (1, 5) and only for them, everything is still removed
    let (code, all) = fake.run(&["--no-kernel", "--link-timeout", "1"]);
    assert_eq!(code, Some(1), "{all}");
    let failed = line(&all, "FAIL", "oper-up");
    assert!(
        failed.contains("port 1: oper not up") && failed.contains("port 5: oper not up"),
        "{failed}"
    );
    assert!(
        !failed.contains("port 121:") && !failed.contains("port 125:"),
        "{failed}"
    );
    assert!(line(&all, "PASS", "teardown").contains("vlan"), "{all}");
    assert_eq!(all.matches("\nFAIL ").count(), 1, "{all}");

    // with it (the fake's diag command, what the SDK's thread does on the switch): all up
    let _ = std::fs::remove_file(&fake.trace);
    let (code, all) = fake.run(&["--no-kernel", "--poke-diag", "fake linkscan"]);
    assert_eq!(code, Some(0), "{all}");
    assert!(
        line(&all, "PASS", "oper-up").contains("ext-phy=the-profile's-table"),
        "{all}"
    );
    let chip = fake.trace();
    // the gates, the attach probe, the table's sequences and the smoke's one read: nothing else
    assert!(chip.contains("gate 1 on\ngate 2 on\n"), "{chip}");
    assert!(chip.contains("R 0 1 0 = 0000"), "the attach probe:\n{chip}");
    let table_writes = [
        "W 10 1 d139 351",
        "W 10 1 d139 350",
        "W 10 1 a002 c161",
        "W 10 1 8222 1111",
    ];
    let writes = mdio_writes(&chip);
    assert!(!writes.is_empty(), "{chip}");
    for w in &writes {
        assert!(
            table_writes.contains(w),
            "an MDIO write that is not the table's: {w}\n{chip}"
        );
    }
    // admin-down at creation, admin-up (after its check read) before the port is enabled, link
    // polls through the diag command, admin-down at the end
    let up = chip
        .find("R 10 1 d0e1 = a008\nW 10 1 d139 350\nport 1 enable 1")
        .unwrap_or_else(|| panic!("{chip}"));
    assert!(chip[..up].contains("W 10 1 d139 351"), "{chip}");
    assert!(chip[up..].contains("shell fake linkscan\n"), "{chip}");
    assert!(chip[up..].contains("R 11 1 a101 = 0003"), "{chip}");
    assert!(chip[up..].contains("port 1 enable 0\nW 10 1 d139 351"), "{chip}");
    let reads: Vec<&str> = chip
        .lines()
        .filter(|l| l.starts_with("R ") && l.contains(" 8b00 "))
        .collect();
    assert_eq!(reads, ["R 0 1 8b00 = 2391"], "{chip}");
    assert!(!chip.lines().any(|l| l.starts_with("W 0 ")), "{chip}");

    // --no-ext-phy: the same profile served without the table: no gate, no MDIO but the smoke's read
    let _ = std::fs::remove_file(&fake.trace);
    let (code, all) = fake.run(&["--no-kernel", "--no-ext-phy"]);
    assert_eq!(code, Some(0), "{all}");
    assert!(
        line(&all, "PASS", "profile").contains("DROPPED(--no-ext-phy):SAI_OPENBCM_EXT_PHY_FILE "),
        "{all}"
    );
    assert!(all.contains("--no-ext-phy: the profile is served WITHOUT"), "{all}");
    assert!(
        line(&all, "PASS", "oper-up").contains("ext-phy=off(--no-ext-phy)"),
        "{all}"
    );
    let chip = fake.trace();
    assert!(!chip.contains("gate "), "{chip}");
    assert_eq!(mdio_writes(&chip), Vec::<&str>::new(), "{chip}");
    let reads: Vec<&str> = chip.lines().filter(|l| l.starts_with("R ")).collect();
    assert_eq!(reads, ["R 0 1 8b00 = 2391"], "{chip}");
}

#[test]
fn the_smoke_program_fails_loudly_and_still_cleans_up() {
    let Some((exe, lib)) = setup() else { return };
    // the wrong port count: the port steps are skipped, the switch removed, exit 1
    let fake = Fake::new("smoke-fail", &exe, &lib, "", "");
    let (code, all) = fake.run(&["--no-kernel", "--expect-ports", "33"]);
    assert_eq!(code, Some(1), "{all}");
    assert!(line(&all, "FAIL", "ports").contains("32 ports, expected 33"), "{all}");
    assert!(line(&all, "SKIP", "admin-up").contains("not as expected"), "{all}");
    assert!(
        all.contains("PASS remove-switch") && all.contains("SMOKE_FAILED"),
        "{all}"
    );
    assert!(fake.trace().ends_with("shutdown unit 0\n"));

    // a wrong MDIO value and a cabled port that is not one: FAIL lines, the rest as usual
    let (code, all) = fake.run(&["--no-kernel", "--mdio-expect", "0x1234"]);
    assert_eq!(code, Some(1), "{all}");
    assert!(
        line(&all, "FAIL", "mdio-read").contains("read 0x2391, expected 0x1234"),
        "{all}"
    );
    assert!(
        all.contains("PASS teardown") && all.contains("PASS admin-down"),
        "{all}"
    );
    let (code, all) = fake.run(&["--no-kernel", "--cabled", "1,7"]);
    assert_eq!(code, Some(1), "{all}");
    assert!(
        line(&all, "FAIL", "ports").contains("cabled port 7: no port has that first lane"),
        "{all}"
    );

    // without --no-kernel (as on the switch) a missing netdev is a FAIL, not a skip
    let (code, all) = fake.run(&["--netdev-timeout", "1", "--hostif-name", "smk-absent0"]);
    assert_eq!(code, Some(1), "{all}");
    assert!(
        line(&all, "FAIL", "hostif-netdev").contains("/sys/class/net/smk-absent0 does not exist"),
        "{all}"
    );
    assert!(line(&all, "PASS", "teardown").contains("hostif"), "{all}");

    // a chip that does not come up: create-switch fails, everything after it is skipped
    let busy = Fake::new("smoke-busy", &exe, &lib, "init busy\n", "");
    let (code, all) = busy.run(&["--no-kernel"]);
    assert_eq!(code, Some(1), "{all}");
    assert!(all.contains("another SDK process owns the switch"), "{all}");
    assert!(line(&all, "FAIL", "create-switch").contains("create_switch:"), "{all}");
    assert!(
        line(&all, "SKIP", "mdio-read").contains("create_switch failed"),
        "{all}"
    );
    assert!(all.contains("PASS uninitialize"), "{all}");
}
