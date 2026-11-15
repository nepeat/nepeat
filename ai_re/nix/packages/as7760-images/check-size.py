#!/usr/bin/env python3
"""Check actual allocated runtime files (not compressed NAR sizes)."""
import json
import sys
from pathlib import Path

limit = 3 * 1024**3
total = 0
for root in Path(sys.argv[1]).read_text().splitlines():
    paths = [Path(root)]
    if Path(root).is_dir():
        paths.extend(Path(root).rglob("*"))
    for path in paths:
        stat = path.lstat()
        # Charge separately even for host hardlinks: copying store paths can
        # lose cross-path deduplication. Include filesystem metadata headroom.
        total += max(4096, (stat.st_size + 4095) // 4096 * 4096)
report = {"closure_bytes_upper_estimate": total, "limit_bytes": limit,
          "within_budget": total <= limit}
print(json.dumps(report, indent=2))
Path(sys.argv[2]).write_text(json.dumps(report, indent=2) + "\n")
if total > limit:
    sys.exit("runtime closure exceeds 3 GiB; do not deploy this image")
