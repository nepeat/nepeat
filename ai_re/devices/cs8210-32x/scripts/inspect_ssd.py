"""Inventory verified image filesystems without replaying their journals.

No programs from the image are executed. Mounts are read-only, nosuid, nodev,
noexec, noload. All mounts and the read-only loop device are cleaned up on exit.
Raw inventories may contain private names and stay in gitignored evidence/.
"""
import json
import os
from pathlib import Path
import subprocess

base = Path(__file__).resolve().parents[1]
os.umask(0o077)
assert json.loads((base / 'evidence/verification.json').read_text())['matches_acquisition']
raw = base / 'analysis/ssd.img'
def run(args, **kwargs):
    return subprocess.run(args, check=True, **kwargs)
loop = subprocess.check_output(['sudo', '-n', 'losetup', '--find', '--show', '--read-only', '--partscan', str(raw)], text=True).strip()
mounted = []
try:
    table = json.loads((base / 'evidence/partition-table.json').read_text())['partitiontable']
    for part in table['partitions']:
        if part['type'] == '5':
            continue
        number = part['node'].removeprefix('/dev/sdb')
        device = loop + 'p' + number
        target = base / ('analysis/mnt-p' + number)
        target.mkdir(exist_ok=True)
        with (base / f'evidence/p{number}-superblock.txt').open('w') as output:
            run(['sudo', '-n', 'dumpe2fs', '-h', device], stdout=output, stderr=subprocess.STDOUT)
        run(['sudo', '-n', 'mount', '-t', 'ext4', '-o', 'ro,noload,nosuid,nodev,noexec', device, str(target)])
        mounted.append(target)
        # find defaults to not following symlinks; don't traverse off-image paths.
        with (base / f'evidence/p{number}-files.txt').open('w') as output:
            run(['sudo', '-n', 'find', str(target), '-xdev', '-printf', '%y %s %P -> %l\n'], stdout=output)
        print(f'p{number}:', flush=True)
        run(['sudo', '-n', 'ls', '-la', str(target)])
finally:
    for target in reversed(mounted):
        run(['sudo', '-n', 'umount', str(target)])
    run(['sudo', '-n', 'losetup', '-d', loop])
