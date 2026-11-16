# bfdd heap growth on the AS7760 (open, 2026-11-15)

Switch soak: bfdd RSS +2.85 MB/min with 2 single-hop BGP BFD sessions at
300 ms (default VRF <-> Vrf-test over the lab cable); FRR MTYPEs flat, mallinfo
"Used ordinary blocks" +3 MiB/min, so plain malloc outside FRR's types
([evidence](bfdd-leak-20261115.txt)). bfdsyncd ruled out on the switch.

## does not reproduce in a VM

`nix/modules/switch/sonic/tests/vm-bfdd-leak.nix`: zebra + mgmtd + bfdd
(+ bgpd) from `sonic.routing.frr`, two sessions over a veth pair between the
default VRF and Vrf-test on one box (the lab-cable loop, local ip rule moved
after l3mdev as vrfmgrd does), or two links to a peer VM without VRF. Checks:
mallinfo heap in use grows < 512 KiB per phase, RSS slope < 30 kB/min.

| run (10 min per phase, 300 ms x3, ~16 pkt/s) | bfdd heap / RSS |
| --- | --- |
| x86, configured sessions, VRF loop | RSS 11384 kB flat |
| x86, configured sessions, no VRF (and the peer's stock nixpkgs FRR) | flat / flat |
| x86, sessions from bgpd (iBGP across the VRFs, as the switch), 3 + 10 min | heap in use 2013 -> 2015 KiB; RSS flat after the first sample |
| **ppc64: the switch's own frr-sonic store path** (53pyiac...) under qemu-user in the VM, bgpd sessions, VRF loop, malloc-trace | heap in use 2016 -> 2018 KiB in 10 min |
| ppc64 under qemu-user, configured sessions, VRF loop / no VRF (with the pass/fail checks) | heap in use 2087936 -> 2087936 / 2147328 -> 2147328 bytes |

Evidence: [bfdd-leak-vm-20261115.txt](bfdd-leak-vm-20261115.txt). So neither x86 nor the identical ppc64 binary leaks with the same sessions.
Not covered by the VM: the real ppc64 kernel and KNET netdevs (bcm-knet), the
trap path, the rest of the SONiC stack (zebra with FPM and 38 ports,
orchagent, ...). Upstream FRR master has no bfdd leak fix after 10.7.1 that
matches (checked the bfdd commit log up to 2026-11-13).

## trace it on the switch

`sonic.routing.malloc-trace` (packages/sonic/routing/malloc-trace): an
LD_PRELOAD that charges every malloc to its call stack and appends the
stacks with the most live bytes to a file every N s. Validated on x86 and on
the switch's ppc64 bfdd under qemu (frames resolve to e.g.
`libfrr.so.0+0x17b888 (stream_new+0x58)`). Overhead: a backtrace per
allocation; fine for bfdd.

```sh
# workstation: build + copy (ppc64 build of the current tree)
nix build --no-link --print-out-paths --impure --expr \
  '(import ./nix/packages/sonic { pkgs = (builtins.getFlake "path:'$PWD'/nix").nixosConfigurations.as7760.pkgs; }).routing.malloc-trace'
#   -> /nix/store/0p15bhj0csbb0ylb43466p0fd1b426zh-malloc-trace-powerpc64-unknown-linux-gnuabielfv2-0.1
nix copy --to ssh://root@10.60.2.253 /nix/store/0p15bhj0csbb0ylb43466p0fd1b426zh-malloc-trace-powerpc64-unknown-linux-gnuabielfv2-0.1

# switch: run bfdd under it (runtime drop-in, gone at reboot)
MT=/nix/store/0p15bhj0csbb0ylb43466p0fd1b426zh-malloc-trace-powerpc64-unknown-linux-gnuabielfv2-0.1/lib/libmalloc-trace.so
mkdir -p /run/systemd/system/sonic-frr-bfdd.service.d
printf '[Service]\nEnvironment=LD_PRELOAD=%s\nEnvironment=MALLOC_TRACE_OUT=/var/tmp/bfdd.mt\nEnvironment=MALLOC_TRACE_INTERVAL=300\nEnvironment=MALLOC_TRACE_TOP=12\n' "$MT" \
  > /run/systemd/system/sonic-frr-bfdd.service.d/malloc-trace.conf
systemctl daemon-reload && systemctl restart sonic-frr-bfdd     # bgpcfgd/bfdsyncd restart with it; sessions re-form
tr '\0' '\n' < /proc/$(pgrep -x bfdd)/environ | grep -c malloc-trace   # 1
head -1 /var/tmp/bfdd.mt                                          # "=== start pid=..."
# after >= 30 min (the leak is ~85 MB by then):
grep '^=== t=' /var/tmp/bfdd.mt          # live_bytes must climb ~2.9 MB/min if the leak is in malloc
awk '/^=== t=/{n++} n>=1' /var/tmp/bfdd.mt | tail -150   # last dump: the top stack(s) with allocs >> frees
# cheap side check: what zebra keeps sending bfdd (interface churn?)
vtysh -c 'show zebra client' | sed -n '/Client: bfd/,/^$/p'; sleep 60; vtysh -c 'show zebra client' | sed -n '/Client: bfd/,/^$/p'
# undo
rm -r /run/systemd/system/sonic-frr-bfdd.service.d; systemctl daemon-reload; systemctl restart sonic-frr-bfdd
```

Send back `/var/tmp/bfdd.mt` (and the two zebra client dumps). Frames are
`object+offset (symbol+off)`; "?" frames in bfdd/libfrr resolve on the
workstation with the unstripped rebuild of the same derivation, whose .text is
byte-identical to the deployed one (checked: bfdd, libfrr.so.0, zebra):
```sh
U=$(nix build --no-link --print-out-paths --impure --expr 'let f = builtins.getFlake "path:'$PWD'/nix"; s = import ./nix/packages/sonic { pkgs = f.nixosConfigurations.as7760.pkgs; }; in s.routing.frr.overrideAttrs (o: { dontStrip = true; })')
powerpc64-unknown-linux-gnuabielfv2-addr2line -f -i -e $U/lib/libfrr.so.0 0x<offset>   # binutils of as7760.pkgs.stdenv.cc
```
If live_bytes stays flat while RSS grows, the growth is not malloc (mmap,
thread stacks, ...): then `cat /proc/$(pgrep -x bfdd)/smaps_rollup` and
`grep -A3 -E '^[0-9a-f]+-' /proc/$(pgrep -x bfdd)/smaps | awk '/Rss/{print $2}' | sort -n | tail`
twice, 10 min apart.

Deploy note: `routing/default.nix` now references `./malloc-trace`; the
git flake deploy.sh uses only sees tracked files: `git add -N
ai_re/nix/packages/sonic/routing/malloc-trace` before deploying.
