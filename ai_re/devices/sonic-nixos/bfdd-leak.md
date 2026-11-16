# bfdd heap growth on the AS7760 (found, fixed in the VM, 2026-11-15)

**Cause:** json-c 0.19, not FRR. Its new non-recursive `json_object_put()`
(json-c #923) frees non-empty containers inside `assert(...)`; nixpkgs builds
json-c with NDEBUG, so every JSON object/array that had members leaks with its
whole tree. bfdsyncd's `show bfd peers json` every 200 ms made bfdd grow
~2.9 MB/min ([soak](bfdd-leak-20261115.txt), [switch trace](bfdd-malloc-trace-20261115.txt),
[resolved stacks, before/after](bfdd-leak-jsonc-20261115.txt)). Every FRR
daemon asked for JSON leaks the same way (bgpd: bgpmon's `show bgp summary
json` every 15 s).

**Fix:** `sonic.routing.json-c` = nixpkgs json-c + upstream f291fa81c6 (PR
#936, free outside assert) and the #934 follow-ups 3aa996b5b6, 0c3a5a1994
(`packages/sonic/routing/patches/json-c-000*`); `sonic.routing.frr` links it.
Drop when nixpkgs' json-c has them. Other users of json-c 0.19 on the system
(none long-running with JSON output found) still get the unpatched library.

**Why the first VM runs were flat:** they had no JSON poller. Why the switch
isolation ("stop bfdsyncd 3 min, no change") misled: orchagent `Upholds` its
sonic-* units, so `systemctl stop sonic-bfdsyncd` is undone at once. To hold
one down: `systemctl mask --runtime --now sonic-bfdsyncd` (unmask after).

## checks

- `tests/vm-bfdd-leak.nix` runs bfdsyncd (dry run, no redis) against bfdd at
  200 ms and fails if bfdd's heap in use grows >= 512 KiB per phase or RSS
  >= 30 kB/min. With `unpatchedJsonC = true`: heap +15 MB in 5 min, RSS
  +2901 kB/min (the switch's rate); patched: 0 bytes in 10 min per phase.
  Args: `minutes`, `mode = "configured" | "bgp"`, `pollMs`, `bfdsyncd`,
  `trace` (malloc-trace), `ppc64Pkgs` (run the switch's ppc64 binaries under
  qemu-user).
- Switch, after deploying: `P=$(pgrep -x bfdd); grep VmRSS /proc/$P/status; sleep 600; grep VmRSS /proc/$P/status`
  (flat within a few hundred kB), `vtysh -c 'show memory bfdd' | grep 'Used ordinary'` twice, and
  the same for bgpd over an hour.

## malloc-trace (kept for the next leak)

`sonic.routing.malloc-trace`: LD_PRELOAD heap profiler (live bytes per call
stack, dumped every `MALLOC_TRACE_INTERVAL` s to `MALLOC_TRACE_OUT`, file
opened before the daemon drops privileges). Run a sonic-* daemon under it with
a runtime drop-in:
```sh
MT=$(nix build --no-link --print-out-paths --impure --expr '(import ./nix/packages/sonic { pkgs = (builtins.getFlake "path:'$PWD'/nix").nixosConfigurations.as7760.pkgs; }).routing.malloc-trace')
nix copy --to ssh://root@10.60.2.253 $MT        # then on the switch:
mkdir -p /run/systemd/system/sonic-frr-bfdd.service.d
printf '[Service]\nEnvironment=LD_PRELOAD=%s/lib/libmalloc-trace.so\nEnvironment=MALLOC_TRACE_OUT=/var/tmp/bfdd.mt\nEnvironment=MALLOC_TRACE_INTERVAL=300\n' $MT \
  > /run/systemd/system/sonic-frr-bfdd.service.d/malloc-trace.conf
systemctl daemon-reload && systemctl restart sonic-frr-bfdd
# undo: rm -r /run/systemd/system/sonic-frr-bfdd.service.d; systemctl daemon-reload; systemctl restart sonic-frr-bfdd
```
"?" frames in FRR resolve with `addr2line` (as7760 cross binutils) on
`s.routing.frr.overrideAttrs (o: { dontStrip = true; })`: same derivation
unstripped, .text byte-identical to the deployed one.

**Verified on the switch (2026-11-15 20:49-20:59):** bfdd RSS 13572 -> 13572 kB, heap 2034 KiB flat, bgpd 23632 kB flat, linked to the patched json-c ([evidence](bfdd-leak-fixed-switch-20261115.txt)).
