#!/usr/bin/env python3
"""Send one paced command to the DMC-12 console and print its output.

Goes through the logged tmux pane `serial-dmc12` (never the UART directly),
waits for the prompt in the pane (tio block-buffers its log file), and prints
what appeared after the command. Default prompt: factory U-Boot `ALPINE_DB>`.
  console.py 'version'
  console.py --prompt '# ' --timeout 60 'cat /proc/mtd'
  console.py --out uboot-env-private.txt printenv   # env can hold secrets
"""
import argparse, pathlib, subprocess, sys, time

SESSION = "serial-dmc12"


def pane():
    return subprocess.run(["tmux", "capture-pane", "-p", "-J", "-S", "-", "-t", SESSION],
                          check=True, capture_output=True, text=True, errors="replace").stdout


parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
parser.add_argument("command")
parser.add_argument("--prompt", default="ALPINE_DB>")
parser.add_argument("--timeout", type=float, default=20)
parser.add_argument("--out", type=pathlib.Path, help="write output here instead of stdout")
args = parser.parse_args()
if "\n" in args.command or "\r" in args.command:
    parser.error("send one command at a time")

subprocess.run(["tmux", "clear-history", "-t", SESSION], check=True)
before = pane()
for byte in args.command.encode():
    # -H keeps ';' literal (tmux treats a bare ';' argument as a separator).
    subprocess.run(["tmux", "send-keys", "-t", SESSION, "-H", f"{byte:02x}"], check=True)
    time.sleep(0.02)
subprocess.run(["tmux", "send-keys", "-t", SESSION, "Enter"], check=True)

deadline = time.monotonic() + args.timeout
base = before.rstrip()
while True:
    text = pane()
    tail = text[len(base):] if text.startswith(base) else text
    # Done when new output ends in the prompt. The echoed command can wrap at
    # 80 columns, so drop the echo by line count, not by matching its text.
    # The last line must end in the prompt, but not as part of U-Boot's "crc32 ... ==> " (printed
    # just before the value: a capture in that gap would end in "=>" too).
    last = tail.rstrip().splitlines()[-1].strip() if tail.strip() else ""
    want = args.prompt.strip()
    if last.endswith(want) and not last.endswith("=" + want) and "\n" in tail.strip("\n"):
        lines = tail.strip("\n").splitlines()
        echo = 0
        joined = ""
        while echo < len(lines) and args.command not in joined:
            joined += lines[echo].strip("\r")
            echo += 1
        output = "\n".join(lines[echo:-1])
        break
    if time.monotonic() >= deadline:
        print(tail, file=sys.stderr)
        raise SystemExit(f"timed out after {args.timeout}s waiting for {args.prompt!r}")
    time.sleep(0.1)

if args.out:
    args.out.write_text(output + "\n")
    print(f"wrote {len(output)} chars to {args.out}")
else:
    print(output)
