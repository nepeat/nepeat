"""Test builds of every U-Boot variant, package, system and image under nix/.

Run with "just test" (tiers: just --list). One test per target, so a failure
names the output; the assertion carries the tail of the build log.
See nixtargets.py for where the targets come from.
"""
import functools
import json
import re
import time

import pytest

import nixtargets as nt

TARGETS = nt.targets()
BY_ID = {t.id: t for t in TARGETS}


def _params(build=False):
    out = []
    for t in TARGETS:
        broken = nt.known_broken(t.id)
        if not build:
            marks = [pytest.mark.xfail(reason=broken, strict=True)] if broken else []
            out.append(pytest.param(t, id=t.id, marks=marks))
        elif not t.error:
            marks = [getattr(pytest.mark, t.tier)]
            if broken:
                marks.append(pytest.mark.skip(reason="known broken: " + broken))
            elif t.id in nt.SKIP:
                marks.append(pytest.mark.skip(reason=nt.SKIP[t.id]))
            out.append(pytest.param(t, id=t.id, marks=marks))
    return out


def _fail(t, what, r, seconds):
    hint = ""
    if t.id.startswith("git:"):
        hint = ("\n(git flake = what deploy.sh builds: tracked files only. "
                "'git add -N' new files; compare with the path:/system: target of the same name.)")
    drvs = nt.failed_derivations(r.stderr)
    pytest.fail(f"{what} {t.id} failed (exit {r.returncode}, {seconds:.0f} s){hint}\n"
                f"$ {' '.join(a if len(a) < 200 else '<prelude> ' + t.ref for a in r.args)}\n"
                + (f"could not build:\n{drvs}\n" if drvs else "")
                + nt.tail(r.stderr), pytrace=False)


# --- eval tier ---------------------------------------------------------------

@pytest.mark.eval
def test_discovery():
    """Every source of targets could be listed, and listed something."""
    d = nt.discovered()
    assert not d["errors"], "\n\n".join(d["errors"])
    kinds = {t.id.split(":")[0] for t in TARGETS}
    assert kinds == {"path", "system", "git", "extra"}, kinds


@pytest.mark.eval
@pytest.mark.parametrize("t", _params())
def test_eval(t):
    """The target evaluates to a derivation (no building)."""
    if t.error:
        pytest.fail(f"{t.id}: {t.error}", pytrace=False)
    t0 = time.time()
    r = nt.run(t.eval_cmd(), nt.EVAL_TIMEOUT)
    if r.returncode != 0:
        _fail(t, "evaluation of", r, time.time() - t0)
    assert re.fullmatch(r"/nix/store/[a-z0-9]{32}-.+\.drv", r.stdout.strip()), r.stdout


@pytest.mark.eval
def test_every_output_has_a_tier():
    """A new uboot-* or package output is built by a tier or skipped with a reason."""
    tiers = {"uboot", "packages", "systems", "deploy", "vm"}
    stray = [t.id for t in TARGETS if t.tier not in tiers]
    assert not stray, stray
    uboot = [t for t in TARGETS if t.id.startswith("path:uboot-")]
    assert uboot and all(t.tier == "uboot" for t in uboot)
    assert any(t.tier == "systems" for t in TARGETS)


@pytest.mark.eval
def test_tables_are_current():
    """SKIP / KNOWN_BROKEN / EXCLUDE entries still name something that exists."""
    stale = [k for k in nt.SKIP if k not in BY_ID]
    assert not stale, f"SKIP entries with no such target (remove them): {stale}"
    stale = [rx for rx in nt.KNOWN_BROKEN if not any(re.fullmatch(rx, i) for i in BY_ID)]
    assert not stale, f"KNOWN_BROKEN patterns that match no target (remove them): {stale}"
    excluded = nt.discovered()["excluded"]
    unused = [rx for rx in nt.EXCLUDE if not any(re.fullmatch(rx, i) for i in excluded)]
    assert not unused, f"EXCLUDE patterns that match nothing (remove them): {unused}"


# U-Boot variant derivations deliberately not exported: "<set>.<name>" -> reason
# (set = "uboot" or the sub-directory of packages/uboot).
UBOOT_NOT_EXPORTED: dict[str, str] = {}


@pytest.mark.eval
def test_uboot_variants_are_exported():
    """Every derivation of a U-Boot variant set is a uboot-* flake output.

    packages/uboot/default.nix and its sub-directories each return a set of
    builds; packages/default.nix exports them by hand. A build added to a set
    but not exported would never be test-built.
    """
    subdirs = sorted(p.parent.name for p in (nt.NIX_DIR / "packages/uboot").glob("*/default.nix"))
    sets = "{ uboot = uboots; " + " ".join(
        f'"{d}" = import (src + "/packages/uboot/{d}") {{ pkgs = p; inherit uboots; }};' for d in subdirs) + " }"
    expr = (f"{nt.PRELUDE} let sets = {sets}; drv = v: (builtins.tryEval ((v.type or \"\") == \"derivation\")).value; in "
            "{ variants = builtins.mapAttrs (_: s: builtins.mapAttrs (_: v: v.drvPath) "
            "(p.lib.filterAttrs (_: v: builtins.isAttrs v && drv v) s)) sets; "
            "exported = builtins.mapAttrs (_: v: v.drvPath) "
            f"(p.lib.filterAttrs (n: _: p.lib.hasPrefix \"uboot-\" n) f.packages.{nt.BUILD_SYSTEM}); }}")
    r = nt.run(nt.nix("eval", "--json", "--impure", "--expr", expr), nt.EVAL_TIMEOUT)
    assert r.returncode == 0, nt.tail(r.stderr)
    d = json.loads(r.stdout)
    assert len(d["variants"]) == len(subdirs) + 1
    exported = set(d["exported"].values())
    found = {f"{s}.{name}": drv for s, variants in d["variants"].items() for name, drv in variants.items()}
    missing = sorted(k for k, drv in found.items() if drv not in exported and k not in UBOOT_NOT_EXPORTED)
    assert not missing, ("U-Boot builds that no uboot-* output exports (add them to nix/packages/default.nix, "
                         f"or to UBOOT_NOT_EXPORTED with a reason): {missing}")
    stale = sorted(k for k in UBOOT_NOT_EXPORTED if k not in found or found[k] in exported)
    assert not stale, f"UBOOT_NOT_EXPORTED entries that are stale: {stale}"


@functools.lru_cache(maxsize=None)
def _closure(target_id):
    """Names of every derivation that building the target needs (nothing is built)."""
    r = nt.run(BY_ID[target_id].eval_cmd(), nt.EVAL_TIMEOUT)
    assert r.returncode == 0, f"{target_id} does not evaluate:\n{nt.tail(r.stderr, 15)}"
    q = nt.run(["nix-store", "--query", "--requisites", r.stdout.strip()], nt.EVAL_TIMEOUT)
    assert q.returncode == 0, nt.tail(q.stderr)
    return q.stdout


def _claim_ok(claim):
    if claim[0] == "none":
        return bool(claim[1])
    if claim[0] == "target":
        return any(re.search(claim[1], t.id) for t in TARGETS if not t.error)
    if claim[0] == "closure":
        return re.search(claim[2], _closure(claim[1])) is not None
    raise AssertionError(f"unknown claim {claim!r}")


@pytest.mark.eval
def test_every_package_dir_is_covered():
    """Each nix/packages/<dir> is claimed by a target (PACKAGE_DIRS), and the claims hold."""
    dirs = sorted(p.name for p in (nt.NIX_DIR / "packages").iterdir() if p.is_dir())
    unclaimed = [d for d in dirs if d not in nt.PACKAGE_DIRS]
    gone = [d for d in nt.PACKAGE_DIRS if d not in dirs]
    assert not unclaimed, f"new nix/packages directories that no test builds: {unclaimed} (add to PACKAGE_DIRS)"
    assert not gone, f"PACKAGE_DIRS entries for directories that no longer exist: {gone}"
    broken = [f"{d}: {c}" for d, claims in nt.PACKAGE_DIRS.items() for c in claims if not _claim_ok(c)]
    assert not broken, f"coverage claims that no longer hold: {broken}"


@pytest.mark.eval
def test_every_test_file_is_built():
    """Each nix file that looks like a test is referenced by a target or listed in TEST_FILES."""
    files = sorted({str(p.relative_to(nt.NIX_DIR)) for g in nt.TEST_FILE_GLOBS for p in nt.NIX_DIR.glob(g)})
    exprs = " ".join(expr for _, expr, _ in nt.EXTRAS.values())
    problems = []
    for f in files:
        base = f.rsplit("/", 1)[-1]
        if ('"/' + f + '"') in exprs or (f.startswith("modules/switch/sonic/tests/") and f'vmTest "{base}"' in exprs):
            continue
        claim = nt.TEST_FILES.get(f)
        if claim is None:
            problems.append(f"{f}: not built by any target (add it to EXTRAS or TEST_FILES)")
            continue
        kind, _, rest = claim.partition(":")
        if kind == "output" and f"path:{rest}" not in BY_ID:
            problems.append(f"{f}: flake output {rest} does not exist")
        elif kind == "extra" and rest not in nt.EXTRAS:
            problems.append(f"{f}: EXTRAS has no {rest}")
        elif kind not in ("output", "extra", "none"):
            problems.append(f"{f}: bad claim {claim!r}")
    problems += [f"{f}: listed in TEST_FILES but gone" for f in nt.TEST_FILES if f not in files]
    assert not problems, "\n".join(problems)


@pytest.mark.eval
def test_every_config_and_board_has_a_system():
    """nix/configs/<dir> is a nixosConfiguration; nix/modules/boards/<dir> has one or is listed."""
    systems = {t.id.split(":", 1)[1] for t in TARGETS if t.id.startswith("system:")}
    assert systems == {t.id.split(".", 1)[1] for t in TARGETS if t.id.startswith("git:nixosConfigurations.")}
    configs = sorted(p.name for p in (nt.NIX_DIR / "configs").iterdir() if p.is_dir())
    boards = sorted(p.name for p in (nt.NIX_DIR / "modules/boards").iterdir() if p.is_dir())
    assert not [c for c in configs if c not in systems], (configs, systems)
    orphans = [b for b in boards if b not in systems and b not in nt.BOARDS_WITHOUT_SYSTEM]
    assert not orphans, f"board modules that no system builds: {orphans} (BOARDS_WITHOUT_SYSTEM)"
    stale = [b for b in nt.BOARDS_WITHOUT_SYSTEM if b in systems or b not in boards]
    assert not stale, f"BOARDS_WITHOUT_SYSTEM entries that are stale: {stale}"


# --- build tiers (marker = the target's tier) --------------------------------

@pytest.mark.parametrize("t", _params(build=True))
def test_build(t):
    """nix build <target> --no-link --print-out-paths."""
    t0 = time.time()
    r = nt.run(t.build_cmd(), nt.BUILD_TIMEOUT)
    if r.returncode != 0:
        _fail(t, "build of", r, time.time() - t0)
    outs = r.stdout.split()
    assert outs and all(o.startswith("/nix/store/") for o in outs), r.stdout
