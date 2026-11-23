#!/usr/bin/env python3
"""Stack budget of a kernel module object (built with -Zemit-stack-sizes).

usage: stack-check.py <llvm-readobj> <llvm-objdump> <llvm-cxxfilt> <obj.o> [--frame N] [--path N]

Reads every function's frame size (.stack_sizes) and the module's own call
graph (call/jump relocations to symbols defined in the object; calls into the
kernel count as 0: their stack is the kernel's), then prints the largest frames
and, for each entry point the kernel calls (probe, ndo/ethtool ops, NAPI poll,
IRQ handlers, module init/exit), the deepest chain of module frames. Fails if
a frame is over --frame bytes or a chain over --path bytes. A recursive cycle
fails too (its depth is unbounded).
"""
import re, subprocess, sys

readobj, objdump, cxxfilt, obj = sys.argv[1:5]
args = sys.argv[5:]
frame_max = int(args[args.index("--frame") + 1]) if "--frame" in args else 1024
path_max = int(args[args.index("--path") + 1]) if "--path" in args else 4096

sizes = {}
out = subprocess.run([readobj, "--stack-sizes", obj], capture_output=True, text=True, check=True).stdout
fn = None
for line in out.splitlines():
    m = re.search(r"Functions: \[(.*)\]", line)
    if m:
        fn = [f.strip() for f in m.group(1).split(",")]
    m = re.search(r"Size: (0x[0-9a-fA-F]+|\d+)", line)
    if m and fn:
        for f in fn:
            sizes[f] = int(m.group(1), 0)
        fn = None
if not sizes:
    sys.exit("stack-check: no .stack_sizes in %s (built without -Zemit-stack-sizes?)" % obj)

calls = {}
cur = None
dis = subprocess.run([objdump, "-dr", "--no-show-raw-insn", obj], capture_output=True, text=True, check=True).stdout
for line in dis.splitlines():
    m = re.match(r"^[0-9a-f]+ <(.+)>:$", line)
    if m:
        cur = m.group(1)
        calls.setdefault(cur, set())
        continue
    m = re.search(r"R_AARCH64_(?:CALL26|JUMP26)\s+(\S+?)(?:\+0x[0-9a-f]+)?$", line)
    if m and cur:
        calls[cur].add(m.group(1))

def demangle(names):
    r = subprocess.run([cxxfilt], input="\n".join(names), capture_output=True, text=True, check=True).stdout
    return dict(zip(names, r.splitlines()))

memo, stack = {}, set()
def depth(f):
    if f in memo:
        return memo[f]
    if f in stack:
        raise RecursionError(f)
    stack.add(f)
    best, via = 0, None
    for c in calls.get(f, ()):
        if c in sizes and c != f:
            d, _ = depth(c)
            if d > best:
                best, via = d, c
    stack.discard(f)
    memo[f] = (sizes.get(f, 0) + best, via)
    return memo[f]

names = demangle(list(sizes))
fail = []
print("largest frames:")
for f, s in sorted(sizes.items(), key=lambda kv: -kv[1])[:12]:
    print(f"  {s:6d}  {names[f][:150]}")
    if s > frame_max:
        fail.append(f"frame {s} > {frame_max}: {names[f][:200]}")

entry = re.compile(r"probe_callback|remove_callback|unbind_callback|OpsTable.*::(open|stop|start_xmit|get_stats64|set_rx_mode|change_mtu|set_mac_address|tx_timeout|set_features)|ethtool::(get_drvinfo|get_link|get_link_ksettings)|poll_cb|handle_irq_callback|init_module|cleanup_module|__.*_init|__.*_exit")
print("deepest chains of module frames from the kernel's entry points:")
try:
    roots = [f for f in sizes if entry.search(names[f]) and "Closure" not in names[f]]
    worst = []
    for f in roots:
        d, _ = depth(f)
        chain, g = [], f
        while g:
            chain.append(g)
            g = memo[g][1]
        worst.append((d, f, chain))
    for d, f, chain in sorted(worst, key=lambda t: -t[0]):
        print(f"  {d:6d}  {names[f][:110]}")
        for g in chain[1:]:
            print(f"            {sizes[g]:5d}  {names[g][:110]}")
        if d > path_max:
            fail.append(f"path {d} > {path_max}: {names[f][:200]}")
except RecursionError as e:
    fail.append(f"recursion through {names.get(str(e.args[0]), e.args[0])}")
if fail:
    print("stack-check: FAIL", *fail, sep="\n  ")
    sys.exit(1)
print(f"stack-check: ok (frames <= {frame_max}, chains <= {path_max} bytes)")
