"""What "just test" builds, and how it is found.

Nothing here lists flake outputs by hand. Targets come from three places:

1. path:./nix   every packages.x86_64-linux.* output and every
                nixosConfigurations.*.config.system.build.toplevel. A path flake
                sees the working tree (untracked files too), so this is the form
                that tests what you are editing.
2. the root flake as a git flake (git+file://<monorepo>?dir=ai_re), the form
                the device deploy.sh scripts use: tracked files only, fand and
                consoled from rust/. All its Linux packages and its systems
                (tier "deploy"). Where both flakes give the same derivation
                the second build is a no-op; they differ when a file is not
                tracked yet or a system uses the rust modules.
3. EXTRAS       things with no flake output (SONiC packages, SAI, SDK library,
                build-time and VM tests): one Nix expression each; attribute
                sets are walked for derivations, so a new package in e.g.
                packages/sonic is picked up without touching this file.

The tables below are the only hand-kept parts: EXTRAS, SKIP, KNOWN_BROKEN,
EXCLUDE and the coverage claims. test_nix_builds.py has guards that fail when one of them goes
stale or when something new under nix/ is claimed by none of them.
"""
from __future__ import annotations

import json
import os
import re
import shlex
import subprocess
from concurrent.futures import ThreadPoolExecutor
from dataclasses import dataclass, asdict
from pathlib import Path

AI_RE = Path(__file__).resolve().parents[1]
NIX_DIR = AI_RE / "nix"
BUILD_SYSTEM = "x86_64-linux"   # as nix/flake.nix

# Extra arguments for every nix build (not for evaluation), e.g.
#   AI_RE_NIX_ARGS="--max-jobs 2 --keep-going"   (just: nix_args=...)
NIX_ARGS = shlex.split(os.environ.get("AI_RE_NIX_ARGS", ""))
BUILD_TIMEOUT = float(os.environ.get("AI_RE_BUILD_TIMEOUT", "0")) or None   # seconds per target
EVAL_TIMEOUT = float(os.environ.get("AI_RE_EVAL_TIMEOUT", "1800"))
LOG_TAIL = int(os.environ.get("AI_RE_LOG_TAIL", "60"))   # lines of a failed derivation's log shown

# Never write nix/flake.lock or the root flake.lock from a test run.
_NIX = ["nix", "--extra-experimental-features", "nix-command flakes"]


def nix(sub: str, *args: str) -> list[str]:
    return _NIX + [sub, "--no-write-lock-file", *args]


def monorepo() -> Path:
    r = subprocess.run(["git", "-C", str(AI_RE), "rev-parse", "--show-toplevel"],
                       capture_output=True, text=True, check=True)
    return Path(r.stdout.strip())


# AI_RE_PATH_FLAKE: evaluate another copy of nix/ (drvpaths.py --flake, for
# comparing a saved tree with the working one); the guards still read NIX_DIR.
PATH_FLAKE = os.environ.get("AI_RE_PATH_FLAKE") or f"path:{NIX_DIR}"


def git_flake() -> str:
    return f"git+file://{monorepo()}?dir={AI_RE.relative_to(monorepo())}"


# --- expressions for things that are not flake outputs ----------------------
# Everything is read from the store copy of the path flake (src), so one run
# builds one consistent snapshot of nix/. Unfree parts (the Broadcom SDK
# library, Marvell's isai binaries) are handled as the package READMEs do:
# NIXPKGS_ALLOW_UNFREE=1 + --impure, and config.allowUnfree for "pu".
PRELUDE = """
let
  f = builtins.getFlake "@FLAKE@";
  src = f.outPath;
  np = f.inputs.nixpkgs;
  p = import np { system = "@SYSTEM@"; };
  pu = import np { system = "@SYSTEM@"; config.allowUnfree = true; };
  sys = f.nixosConfigurations;
  sonic = import (src + "/packages/sonic") { pkgs = p; };
  saiFake = p.callPackage (src + "/packages/sai-openbcm") { inherit sonic; };
  # the AS7760's package set (ppc64 big endian, cross-built)
  ppc = sys.as7760.pkgs;
  ppcSonic = import (src + "/packages/sonic") { pkgs = ppc; };
  ppcOpenbcm = import (src + "/packages/openbcm") { pkgs = ppc; };
  ppcSai = ppc.callPackage (src + "/packages/sai-openbcm") { sonic = ppcSonic; libsdk = ppcOpenbcm.libsdk { }; };
  qemuIpd = import (src + "/packages/qemu-ipd") { pkgs = pu; };
  uboots = import (src + "/packages/uboot") { pkgs = p; };
  vmTest = name: import (src + "/modules/switch/sonic/tests/" + name) { pkgs = p; };
  # a board's sonic_platform package on the build host (its fake-board unit tests: .tests.unit)
  platformPkg = board: p.callPackage (src + "/modules/boards/" + board + "/sonic-platform-package.nix") { inherit sonic; };
  # The Rust SAI (rust/sai of the monorepo: its own cargo workspace, outside path:./nix, so it
  # is read from the working tree; every EXTRAS evaluation is --impure). SAI headers: the
  # sairedis pin; the C MDIO interpreter its Rust port is compared with: sai-openbcm's.
  saiRsDir = builtins.dirOf @SAI_RS@;
  saiRs = pk: pk.callPackage @SAI_RS@ {
    saiSrc = (import (src + "/packages/sonic") { pkgs = pk; }).sources.sairedis;
    mdioScriptC = builtins.path {
      path = src + "/packages/sai-openbcm/src"; name = "mdio-script-c";
      filter = path: type: builtins.elem (baseNameOf path) [ "mdio_script.c" "mdio_script.h" "mdio_script_main.c" "mdio_report.c" "mdio_report.h" "temp_files.c" "temp_files.h" "sdk_glue.h" "openbcm_backend.cpp" ];
    };
    saiOpenbcmSrc = builtins.path {
      path = src + "/packages/sai-openbcm/src"; name = "sai-openbcm-src";
      filter = path: type: builtins.match ".*\\.(cpp|h|c)" (baseNameOf path) != null;
    };
  };
  # the boot-time MDIO tool in Rust: native (diag socket + fake glue tests, compared with the C
  # tool), ppc64 with the real glue linked against libsdk, and the units VM test run with it
  mdioToolRs = p.callPackage (saiRsDir + "/mdio-tool.nix") { saiOpenbcmSrc = src + "/packages/sai-openbcm/src"; };
  as7760MdioToolRs = ppc.callPackage (saiRsDir + "/mdio-tool.nix") { libsdk = ppcOpenbcm.libsdk { }; saiOpenbcmSrc = src + "/packages/sai-openbcm/src"; };
  # the Rust libsai with the Broadcom-SDK backend (fake glue natively, libsdk for ppc64)
  saiRsOpenbcm = pk: args: pk.callPackage (saiRsDir + "/libsai-openbcm.nix") ({ saiRs = saiRs pk; } // args);
  # the external-PHY sequence engine in Rust (phy-seq + its C interface as a static library),
  # the C++ libsai built on it, and the AS7772 system with that libsai selected
  physeq = pk: args: pk.callPackage (saiRsDir + "/phy-seq.nix") args;
  saiFakeRust = p.callPackage (src + "/packages/sai-openbcm") { inherit sonic; rustPhySeq = physeq p { }; };
  ppcSaiRust = ppc.callPackage (src + "/packages/sai-openbcm") { sonic = ppcSonic; libsdk = ppcOpenbcm.libsdk { }; rustPhySeq = physeq ppc { }; };
  as7772Rust = sys.as7772.extendModules { modules = [ { hardware.openbcm.rustPhySeq = saiRsDir + "/phy-seq.nix"; } ]; };
  # the SAI MACsec object model in Rust behind its C interface (also carries phy-seq's), the
  # C++ libsai with the model / with model and engine in Rust, and the AS7772 system on both
  macsecCapi = pk: args: pk.callPackage (saiRsDir + "/sai-macsec-capi.nix") ({ saiSrc = (import (src + "/packages/sonic") { pkgs = pk; }).sources.sairedis; } // args);
  saiOpenbcmSrcAll = builtins.path { path = src + "/packages/sai-openbcm/src"; name = "sai-openbcm-src"; filter = path: type: builtins.match ".*\\.(cpp|h|c)" (baseNameOf path) != null; };
  saiFakeRustMacsec = p.callPackage (src + "/packages/sai-openbcm") { inherit sonic; rustMacsec = macsecCapi p { }; };
  saiFakeRustBoth = p.callPackage (src + "/packages/sai-openbcm") { inherit sonic; rustPhySeq = physeq p { }; rustMacsec = macsecCapi p { }; };
  ppcSaiRustBoth = ppc.callPackage (src + "/packages/sai-openbcm") { sonic = ppcSonic; libsdk = ppcOpenbcm.libsdk { }; rustPhySeq = physeq ppc { }; rustMacsec = macsecCapi ppc { }; };
  as7772RustBoth = sys.as7772.extendModules { modules = [ { hardware.openbcm.rustPhySeq = saiRsDir + "/phy-seq.nix"; hardware.openbcm.rustMacsec = saiRsDir + "/sai-macsec-capi.nix"; } ]; };
  # attribute paths of the derivations in v, at most depth levels down. A value
  # that throws is kept as a path, so its evaluation test fails and names it.
  collect = depth: prefix: v:
    let t = builtins.tryEval (if builtins.isAttrs v then (if (v.type or "") == "derivation" then "drv" else "set") else "other");
    in if !t.success || t.value == "drv" then [ prefix ]
       else if t.value == "set" && depth > 0
       then builtins.concatLists (map (n: collect (depth - 1) (prefix ++ [ n ]) v.${n}) (builtins.attrNames v))
       else [ ];
in
""".replace("@FLAKE@", PATH_FLAKE).replace("@SYSTEM@", BUILD_SYSTEM).replace(
    "@SAI_RS@", str(AI_RE.parent / "rust" / "sai" / "nix" / "package.nix"))

# id -> (tier, expression, depth). depth 0: the expression is one derivation;
# depth n: an attribute set walked n levels for derivations (ids id.attr.path).
EXTRAS: dict[str, tuple[str, str, int]] = {
    # SONiC stack, native (the VM tests and the build-host tests use it; the
    # ppc64 one is built by as7760-system).
    "sonic": ("packages", "sonic", 2),
    # libsai on the fake backend + its unit tests, and syncd linked against it.
    "sai-openbcm-fake": ("packages", "saiFake", 0),
    "sairedis-openbcm-fake": ("packages", "sonic.sairedis-openbcm saiFake", 0),
    # Broadcom SDK userspace library, libsai on it, syncd on that (ppc64, unfree).
    "as7760-openbcm-libsdk": ("packages", "ppcOpenbcm.libsdk { }", 0),
    "as7760-sai-openbcm": ("packages", "ppcSai", 0),
    "as7760-sairedis-openbcm": ("packages", "ppcSonic.sairedis-openbcm ppcSai", 0),
    # The Rust SAI workspace (rust/sai: libsai.so on the virtual backend + the other crates): native with fmt, clippy, unit,
    # property and C-harness tests and the MDIO interpreter compared with the C one; for the
    # AS7760's big-endian ppc64 a cross build (bindings checked against the cross gcc).
    "sai-rs": ("packages", "saiRs p", 0),
    "as7760-sai-rs": ("packages", "saiRs ppc", 0),
    # every test executable of that workspace cross-built for ppc64 with its own loader and
    # libc, to run on a switch ("just sai-rs-test-on <host>"); building it executes nothing.
    "as7760-sai-rs-target-tests": ("packages", 'ppc.callPackage (saiRsDir + "/target-tests.nix") { saiRs = saiRs ppc; macsecTable = sys.as7772.config.services.sonic.sai.profile.SAI_OPENBCM_EXT_MACSEC_FILE; }', 0),
    "mdio-tool-rs": ("packages", "mdioToolRs", 0),
    "as7760-mdio-tool-rs": ("packages", "as7760MdioToolRs", 0),
    # Rust libsai with the openbcm backend: on the fake glue (native: backend + glue tests, the C
    # smoke program against the built library) and cross-linked against libsdk for ppc64 (link
    # with -z defs, soname, exports = the virtual build's; nothing executed).
    "sai-rs-openbcm-fake": ("packages", "saiRsOpenbcm p { }", 0),
    "as7760-sai-rs-openbcm": ("packages", 'saiRsOpenbcm ppc { libsdk = ppcOpenbcm.libsdk { }; glueSrc = src + "/packages/sai-openbcm/src"; }', 0),
    # phy-seq: the sequence engine with its tests, compared with the C++ tools over the AS7772's
    # tables and generated ones; the C++ libsai on the Rust engine with test_sai unchanged; ppc64.
    "phy-seq": ("packages", 'physeq p { cxxTools = saiFake; tables = [ sys.as7772.config.services.sonic.sai.profile.SAI_OPENBCM_EXT_MACSEC_FILE sys.as7772.config.services.sonic.sai.profile.SAI_OPENBCM_EXT_PHY_FILE ]; }', 0),
    "as7760-phy-seq": ("packages", "physeq ppc { }", 0),
    "sai-openbcm-fake-rust-phy-seq": ("packages", "saiFakeRust", 0),
    "as7760-sai-openbcm-rust-phy-seq": ("packages", "ppcSaiRust", 0),
    "as7772-system-rust-phy-seq": ("systems", "as7772Rust.config.system.build.toplevel", 0),
    # the MACsec object model in Rust: its tests incl. the full Rust path against the full C++
    # path at MDIO level over the AS7772 table; the C++ libsai with it (test_sai unchanged)
    "sai-macsec-capi": ("packages", 'macsecCapi p { saiOpenbcmSrc = saiOpenbcmSrcAll; tables = [ sys.as7772.config.services.sonic.sai.profile.SAI_OPENBCM_EXT_MACSEC_FILE ]; }', 0),
    "sai-openbcm-fake-rust-macsec": ("packages", "saiFakeRustMacsec", 0),
    "sai-openbcm-fake-rust-both": ("packages", "saiFakeRustBoth", 0),
    "as7760-sai-openbcm-rust-both": ("packages", "ppcSaiRustBoth", 0),
    "as7772-system-rust-both": ("systems", "as7772RustBoth.config.system.build.toplevel", 0),
    # Rust in the kernel (arm64): the DMC-12 kernel recipe with CONFIG_RUST (+ QEMU virt bits),
    # dmc12-cpld-reset in C and in Rust against it (the Rust build runs its logic's unit tests),
    # and both run in QEMU: Rust sample module loads, same SMBus writes and fallbacks, 4 scenarios.
    "kernel-rust": ("packages", 'import (src + "/packages/kernel-rust") { pkgs = p; }', 1),
    # al_eth pieces in Rust, host-tested: the board-parameter registers against 400k vectors
    # from the C HAL functions themselves; no_std, unsafe forbidden.
    "al-eth-rs": ("packages", 'p.callPackage (src + "/packages/al-eth/rust.nix") { }', 0),
    # Teralynx 7 libsai (CS8210), native build with its checkPhase.
    "sai-teralynx": ("packages", 'p.callPackage (src + "/packages/sai-teralynx") { inherit sonic; }', 0),
    # qemu-user for Marvell's amd64 SAI on the CS8210 + its offline tests
    # (fetches Marvell's published binaries into the store: unfree).
    "qemu-ipd": ("packages", "qemuIpd", 2),
    # sonic_platform packages and their fake-board unit tests.
    "as7760-sonic-platform": ("packages", 'platformPkg "as7760"', 0),
    "as7760-sonic-platform-test": ("packages", '(platformPkg "as7760").tests.unit', 0),
    "as7772-sonic-platform-test": ("packages", 'sys.as7772.config.services.sonic.pmon.platformPackage.tests.unit', 0),
    "cs8210-sonic-platform": ("packages", 'platformPkg "cs8210"', 0),
    "cs8210-sonic-platform-test": ("packages", '(platformPkg "cs8210").tests.unit', 0),
    # Build-time tests that are not flake outputs.
    "as7760-sdk-config-test": ("packages", 'import (src + "/modules/boards/as7760/sdk-config-test.nix") { pkgs = p; system = sys.as7760; }', 1),
    "as7772-sdk-config-test": ("packages", 'import (src + "/modules/boards/as7772/sdk-config-test.nix") { pkgs = p; system = sys.as7772; }', 1),
    "cs8210-sdk-config-test": ("packages", 'import (src + "/modules/boards/cs8210/sdk-config-test.nix") { pkgs = p; }', 1),
    "sonic-metrics-test": ("packages", 'p.callPackage (src + "/packages/sonic/metrics/test.nix") { }', 0),
    "sonic-module-eval-test": ("packages", 'import (src + "/modules/switch/sonic/tests/eval.nix") { pkgs = p; }', 1),
    # NixOS VM tests (kvm).
    "vm-sonic": ("vm", 'vmTest "vm.nix"', 0),
    "vm-sonic-frr": ("vm", 'vmTest "vm-frr.nix"', 0),
    "vm-sonic-bfd": ("vm", 'vmTest "vm-bfd.nix"', 0),
    "vm-sonic-metrics": ("vm", 'vmTest "vm-metrics.nix"', 0),
    "vm-openbcm-mdio-units": ("vm", 'import (src + "/packages/sai-openbcm/test-mdio-units.nix") { pkgs = p; }', 0),
    "vm-sonic-macsec": ("vm", 'vmTest "vm-macsec.nix"', 0),
    "vm-sonic-platform": ("vm", 'vmTest "vm-platform.nix"', 0),
    "vm-sonic-openbcm-fake": ("vm", 'vmTest "vm-openbcm-fake.nix"', 0),
    "vm-sonic-openbcm-fake-acl": ("vm", 'vmTest "vm-openbcm-fake-acl.nix"', 0),
    "vm-sonic-openbcm-fake-l3": ("vm", 'vmTest "vm-openbcm-fake-l3.nix"', 0),
    "vm-sonic-bfdd-leak": ("vm", 'vmTest "vm-bfdd-leak.nix"', 0),
    "vm-sai-openbcm-syncd": ("vm", 'import (src + "/packages/sai-openbcm/test-syncd.nix") { pkgs = p; }', 0),
    # the MDIO units VM test (vm-openbcm-mdio-units, unmodified) with the Rust tool as the binary
    "vm-mdio-units-rs": ("vm", 'import (saiRsDir + "/test-mdio-units.nix") { pkgs = p; aiReNix = src; }', 0),
    # sai-openbcm's own smoke test (sai_smoke.py, unchanged) through syncd on the Rust libsai,
    # and the full SONiC fake-backend tests on it (TAP netdevs; the ACL one fails by design: SKIP)
    "vm-sai-rs-syncd-parity": ("vm", 'import (saiRsDir + "/test-syncd-parity.nix") { pkgs = p; inherit sonic; libsai = saiRs p; smoke = src + "/packages/sai-openbcm/tests/sai_smoke.py"; }', 0),
    "vm-sai-rs-sonic": ("vm", 'import (saiRsDir + "/test-sonic-virtual.nix") { pkgs = p; inherit sonic; libsai = saiRs p; fakeTest = src + "/modules/switch/sonic/tests/vm-openbcm-fake.nix"; }', 0),
    "vm-sai-rs-sonic-l3": ("vm", 'import (saiRsDir + "/test-sonic-virtual-l3.nix") { pkgs = p; inherit sonic; libsai = saiRs p; fakeTest = src + "/modules/switch/sonic/tests/vm-openbcm-fake-l3.nix"; }', 0),
    "vm-sai-rs-sonic-acl": ("vm", 'import (saiRsDir + "/test-sonic-virtual-acl.nix") { pkgs = p; inherit sonic; libsai = saiRs p; fakeTest = src + "/modules/switch/sonic/tests/vm-openbcm-fake-acl.nix"; }', 0),
    # the tests that create netdevs (TAP), as root inside a VM
    "vm-sai-rs-privileged": ("vm", 'import (saiRsDir + "/test-privileged.nix") { pkgs = p; saiRs = saiRs p; }', 0),
    # syncd linked against the Rust libsai (virtual backend), its first slice through libsairedis.
    "vm-sai-rs-syncd": ("vm", 'import (saiRsDir + "/test-syncd.nix") { pkgs = p; inherit sonic; libsai = saiRs p; }', 0),
}

# Discovered ids (regex, full match) that are not targets at all.
EXCLUDE: dict[str, str] = {
    r"extra:sonic\.sources\..*": "pinned source fetches; every consumer builds them",
    r"extra:sonic\.libnl-sonic": "nixpkgs' libnl under another name",
    r"extra:sonic\.python\.python3": "nixpkgs' python interpreter, not ours",
}

# Targets that are evaluated but not built (id -> reason). Keep it short: an
# entry here is a build nobody checks. A stale entry fails test_tables_are_current.
SKIP: dict[str, str] = {
    "extra:vm-sonic-bfdd-leak": "a 2 x 10 min memory soak (an experiment, not a regression test)",
    "extra:vm-sai-rs-sonic-acl": "fails by design: the ACL API is not in the Rust SAI slice (create_acl_table: NOT_IMPLEMENTED; rust/sai README, Parity)",
    "git:packages.aarch64-linux.pyghidra-mcp": "devshell tool, only verified on x86_64-linux (flake.nix)",
}

# Targets known not to evaluate (id regex, full match -> reason). Their eval
# test is a strict xfail: it shows in the summary as xfailed, and fails the run
# as soon as the target evaluates again, so the entry has to be removed. Their
# build test is skipped. This is a list of open bugs and known limits, not a
# place to hide a new failure.
KNOWN_BROKEN: dict[str, str] = {
    r"(path:dmc12-(?!metrics-test).*|system:dmc12(-netboot)?)":
        "limit of path:./nix: the DMC-12 config needs services.consoled from rust/, which only the root git flake "
        "passes (nix/README.md, 'fand and path:./nix'); built as git:...dmc12*",
}


def known_broken(tid: str) -> str | None:
    for rx, reason in KNOWN_BROKEN.items():
        if re.fullmatch(rx, tid):
            return reason
    return None


# Flake output name -> tier. First match wins; the rest are "packages".
TIER_RULES: list[tuple[str, str]] = [
    (r"uboot-.*", "uboot"),
    (r".*-(system|netboot|boot-files|ram-probe|size-check)", "systems"),
    (r".*-(sd|disk)-image(-.*)?", "systems"),
]


def tier_of(output: str) -> str:
    for rx, tier in TIER_RULES:
        if re.fullmatch(rx, output):
            return tier
    return "packages"


# --- coverage claims (checked by the guards) --------------------------------
# Every directory in nix/packages must be claimed, so a new one cannot go
# untested unnoticed. A claim is a list of:
#   ("target", regex)               some discovered target id matches (search)
#   ("closure", target id, regex)   a derivation in that target's build closure
#                                   matches (building the target builds it)
#   ("none", reason)                deliberately not built
_DMC12 = "git:nixosConfigurations.dmc12"
_AS7760 = "system:as7760"
PACKAGE_DIRS: dict[str, list[tuple]] = {
    "uboot": [("target", r"^path:uboot-")],
    # scripts, no derivation of their own; the stage3app builds run them
    "alboot": [("target", r"^path:uboot-dmc12-stage3app$"), ("target", r"^path:uboot-cs8210-stage3app$")],
    "al-eth": [("closure", _DMC12, r"-al_eth-"), ("target", r"^extra:al-eth-rs$")],
    "linux-alpine": [("target", r"^git:packages\.x86_64-linux\.dmc12-kernel$")],
    "linux-qoriq-t2080": [("target", r"^path:as7760-kernel$"), ("target", r"^path:as7760-kernel-image$")],
    "as7760-fan": [("target", r"^path:as7760-fan-driver$"), ("closure", _AS7760, r"-as7760-fan-")],
    "as7760-images": [("target", r"^path:as7760-(boot-files|netboot|sd-image-ram|ram-probe|size-check)$")],
    "as7760-metrics": [("target", r"^path:as7760-metrics-test$"), ("closure", _AS7760, r"-as7760-metrics")],
    "as7760-qsfp": [("closure", _AS7760, r"-as7760-qsfp")],
    "dmc12-cpld-reset": [("closure", _DMC12, r"-dmc12-cpld-reset-"), ("target", r"^extra:kernel-rust\.dmc12-cpld-reset-rs$")],
    "kernel-rust": [("target", r"^extra:kernel-rust\.dmc12-cpld-reset-qemu-test$")],
    "dmc12-firmware": [("target", r"^git:packages\.aarch64-linux\.dmc12-firmware$"), ("closure", _DMC12, r"-dmc12-firmware")],
    "dmc12-images": [("target", r"^git:packages\.x86_64-linux\.dmc12-netboot$")],
    "dmc12-metrics": [("target", r"^path:dmc12-metrics-test$"), ("closure", _DMC12, r"-dmc12-metrics")],
    "fastfetch-tiny": [("closure", _AS7760, r"-fastfetch-tiny-"), ("closure", _DMC12, r"-fastfetch-tiny-")],
    "node-exporter-lite": [("target", r"^path:node-exporter-lite-test$"), ("closure", _AS7760, r"-node-exporter-lite")],
    "openbcm": [("target", r"^path:as7760-openbcm-kmods$"), ("target", r"^path:as7760-bcm-user$"),
                ("target", r"^extra:as7760-openbcm-libsdk$"),
                # configs/: the generator on both boards that use it
                ("target", r"^extra:as7760-sdk-config-test$"), ("target", r"^extra:as7772-sdk-config-test$")],
    "sai-openbcm": [("target", r"^extra:sai-openbcm-fake$"), ("target", r"^extra:as7760-sai-openbcm$"),
                    ("target", r"^extra:vm-sai-openbcm-syncd$"), ("target", r"^extra:vm-openbcm-mdio-units$")],
    "sai-teralynx": [("target", r"^extra:sai-teralynx$"), ("target", r"^extra:cs8210-sdk-config-test$")],
    "isai-amd64": [("target", r"^extra:qemu-ipd\.libsai$")],
    "qemu-ipd": [("target", r"^extra:qemu-ipd\.qemu-ipd$"), ("target", r"^extra:qemu-ipd\.tests\.")],
    "sonic": [("target", r"^extra:sonic\.swss$"), ("target", r"^path:sonic-swss-common$")],
}

# Every nix file that looks like a test must be built by a target or listed
# here. Value: "output:<flake output>", "extra:<EXTRAS id>" (for files reached
# through another expression) or "none:<reason>".
TEST_FILES: dict[str, str] = {
    "packages/uboot/tests.nix": "output:uboot-tests",
    "packages/uboot/alpine-v2/tests.nix": "output:uboot-alpine-v2-tests",
    "packages/uboot/alpine-v3/tests.nix": "output:uboot-alpine-v3-tests",
    "packages/node-exporter-lite/test.nix": "output:node-exporter-lite-test",
    "packages/as7760-metrics/test.nix": "output:as7760-metrics-test",
    "packages/dmc12-metrics/test.nix": "output:dmc12-metrics-test",
    "packages/qemu-ipd/tests/default.nix": "extra:qemu-ipd",
    "modules/switch/sonic/tests/common.nix": "none:shared switch config of the VM tests, not a test",
    "modules/switch/sonic/tests/peer.nix": "none:peer node of the VM tests, not a test",
}
TEST_FILE_GLOBS = ["**/*test*.nix", "**/tests.nix", "**/tests/*.nix"]

# Board modules with no system in nix/configs (so no system build covers them).
BOARDS_WITHOUT_SYSTEM: dict[str, str] = {
    "cs8210": "its system lives in the owner's nepeat/nix flake; here: cs8210-sdk-config-test, cs8210-sonic-platform(-test)",
}


@dataclass(frozen=True)
class Target:
    id: str          # "path:<output>", "system:<name>", "git:<attr path>", "extra:<id>"
    tier: str        # uboot | packages | systems | deploy | vm
    kind: str        # "flake" | "expr"
    ref: str         # installable, or a Nix expression (without the prelude)
    error: str = ""  # discovery of this target failed: the eval test reports it

    def eval_cmd(self) -> list[str]:
        if self.kind == "flake":
            return nix("eval", "--raw", self.ref + ".drvPath")
        return nix("eval", "--raw", "--impure", "--expr", f"{PRELUDE} ({self.ref}).drvPath")

    def build_cmd(self) -> list[str]:
        # No -L: with parallel builds the interleaved log buries the error.
        # Without it nix prints, per failed derivation, its last log-lines lines.
        common = ["--no-link", "--print-out-paths", "--option", "log-lines", str(LOG_TAIL)] + NIX_ARGS
        if self.kind == "flake":
            return nix("build", self.ref, *common)
        return nix("build", "--impure", "--expr", f"{PRELUDE} {self.ref}", *common)


def nix_env() -> dict[str, str]:
    env = dict(os.environ)
    env["NIXPKGS_ALLOW_UNFREE"] = "1"   # only read by --impure evaluations (EXTRAS)
    return env


def run(cmd: list[str], timeout: float | None = None) -> subprocess.CompletedProcess:
    try:
        return subprocess.run(cmd, cwd=AI_RE, env=nix_env(), capture_output=True, text=True, timeout=timeout)
    except subprocess.TimeoutExpired as e:
        def s(b):
            return b.decode(errors="replace") if isinstance(b, bytes) else (b or "")
        return subprocess.CompletedProcess(cmd, 124, s(e.stdout), s(e.stderr) + f"\n[timeout after {timeout} s]")


def tail(text: str, n: int = 3 * LOG_TAIL) -> str:
    lines = text.rstrip().splitlines()
    return "\n".join((["[... %d earlier lines]" % (len(lines) - n)] if len(lines) > n else []) + lines[-n:])


def failed_derivations(text: str) -> str:
    """The derivations nix says it could not build: the first is the cause, the rest depend on it."""
    drvs = re.findall(r"error: Cannot build '(/nix/store/[^']+\.drv)'\.\s+Reason: ([^\n]+)", text)
    return "\n".join(f"  {d}: {why}" for d, why in drvs)


def _attr_names(ref: str) -> list[str]:
    r = run(nix("eval", "--json", ref, "--apply", "builtins.attrNames"), EVAL_TIMEOUT)
    if r.returncode != 0:
        raise RuntimeError(f"nix eval {ref} (attrNames) failed:\n{tail(r.stderr)}")
    return json.loads(r.stdout)


def _quote(path: list[str]) -> str:
    return "".join('."%s"' % a for a in path)


def _discover_extra(item) -> list[Target]:
    eid, (tier, expr, depth) = item
    if depth == 0:
        return [Target(f"extra:{eid}", tier, "expr", expr)]
    r = run(nix("eval", "--json", "--impure", "--expr", f"{PRELUDE} collect {depth} [ ] ({expr})"), EVAL_TIMEOUT)
    if r.returncode != 0:
        return [Target(f"extra:{eid}", tier, "expr", expr, error=f"walking {expr!r} for derivations failed:\n{tail(r.stderr)}")]
    out = []
    for path in sorted(json.loads(r.stdout)):
        tid = f"extra:{eid}" + "".join("." + a for a in path)
        out.append(Target(tid, tier, "expr", f"({expr}){_quote(path)}"))
    if not out:
        out = [Target(f"extra:{eid}", tier, "expr", expr, error=f"{expr!r} contains no derivation (depth {depth})")]
    return out


def discover() -> dict:
    """{"targets": [...], "errors": [...]}; errors are whole sources that could not be listed."""
    targets: list[Target] = []
    errors: list[str] = []
    gf = git_flake()

    def path_packages():
        names = _attr_names(f"{PATH_FLAKE}#packages.{BUILD_SYSTEM}")
        return [Target(f"path:{n}", tier_of(n), "flake", f'{PATH_FLAKE}#packages.{BUILD_SYSTEM}."{n}"') for n in names]

    def path_systems():
        return [Target(f"system:{n}", "systems", "flake",
                       f'{PATH_FLAKE}#nixosConfigurations."{n}".config.system.build.toplevel')
                for n in _attr_names(f"{PATH_FLAKE}#nixosConfigurations")]

    def git_systems():
        return [Target(f"git:nixosConfigurations.{n}", "deploy", "flake",
                       f'{gf}#nixosConfigurations."{n}".config.system.build.toplevel')
                for n in _attr_names(f"{gf}#nixosConfigurations")]

    def git_packages():
        out = []
        for system in _attr_names(f"{gf}#packages"):
            if not system.endswith("-linux"):
                continue   # no darwin builder in the lab (and nixpkgs dropped x86_64-darwin: it does not evaluate)
            for n in _attr_names(f"{gf}#packages.{system}"):
                tier = "deploy"
                out.append(Target(f"git:packages.{system}.{n}", tier, "flake", f'{gf}#packages.{system}."{n}"'))
        return out

    with ThreadPoolExecutor(8) as pool:
        futs = [pool.submit(fn) for fn in (path_packages, path_systems, git_systems, git_packages)]
        extra = list(pool.map(_discover_extra, EXTRAS.items()))
    for fut in futs:
        try:
            targets += fut.result()
        except Exception as e:   # noqa: BLE001 - reported by test_discovery
            errors.append(str(e))
    for group in extra:
        targets += group
    excluded = [t.id for t in targets if any(re.fullmatch(rx, t.id) for rx in EXCLUDE)]
    targets = [t for t in targets if t.id not in excluded]
    return {"targets": [asdict(t) for t in sorted(targets, key=lambda t: t.id)], "errors": errors, "excluded": excluded}


_cache: dict | None = None


def discovered() -> dict:
    """Discovery, once per run: the xdist controller stores it in a file for its workers."""
    global _cache
    if _cache is None:
        path = os.environ.get("AI_RE_DISCOVERY_FILE")
        if path and os.path.exists(path):
            _cache = json.loads(Path(path).read_text())
        else:
            _cache = discover()
            if path:
                Path(path).write_text(json.dumps(_cache))
    return _cache


def targets() -> list[Target]:
    return [Target(**t) for t in discovered()["targets"]]
