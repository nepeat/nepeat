#!/usr/bin/env python3
"""Boot the candidate on QEMU virt. This does NOT emulate the CS8210 board."""
import json
from pathlib import Path
import select
import subprocess
import time

out = Path(__file__).resolve().parents[1] / 'analysis/research/serial-build'
cmd = ['qemu-system-aarch64', '-machine', 'virt,gic-version=3,virtualization=on',
       '-cpu', 'cortex-a72', '-smp', '4', '-m', '512M', '-nographic', '-monitor', 'none',
       '-no-reboot', '-nic', 'none', '-kernel', str(out / 'arch/arm64/boot/Image'),
       '-append', 'console=ttyAMA0 earlycon=pl011,0x9000000 rdinit=/init panic=0']
process = subprocess.Popen(cmd, stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.STDOUT)
transcript = bytearray()
deadline = time.monotonic() + 45
sent = False
success = False
try:
    while time.monotonic() < deadline:
        if select.select([process.stdout], [], [], 1)[0]:
            chunk = process.stdout.read1(65536)
            if not chunk:
                break
            transcript.extend(chunk)
        if b'Kernel panic' in transcript:
            break
        if b' # ' in transcript and not sent:
            process.stdin.write(b'echo SMOKE_BEGIN; uname -r; cat /proc/uptime; ls /sys/devices/system/cpu; echo SMOKE_OK\n')
            process.stdin.flush()
            sent = True
        if b'cpu0' in transcript and b'\r\nSMOKE_OK\r\n' in transcript:
            success = True
            break
finally:
    process.terminate()
    try:
        tail = process.communicate(timeout=5)[0]
    except subprocess.TimeoutExpired:
        process.kill()
        tail = process.communicate()[0]
    transcript.extend(tail)
    (out / 'qemu-smoke.txt').write_bytes(transcript)
    (out / 'qemu-command.json').write_text(json.dumps(cmd, indent=2) + '\n')
    (out / 'qemu-result.json').write_text(json.dumps({'success': success,
        'scope': 'QEMU virt: kernel, initramfs, ELF loader, interactive shell, SMP; not CS8210 hardware or board DTB'}, indent=2) + '\n')
print(transcript.decode(errors='replace')[-2500:])
raise SystemExit(0 if success else 1)
