#!/usr/bin/env python3
"""First-run defaults for the measured TSPS software-decoder budget.

Existing settings, host presets and explicit UI choices remain authoritative.
This is a throughput mitigation, not a hardware decoder or a promise of 60 FPS.
"""
import json
from pathlib import Path
import sys

p = Path(sys.argv[1])
p.parent.mkdir(parents=True, exist_ok=True)
try:
    with p.open('x') as f:
        json.dump({'width': 1280, 'height': 720, 'refresh_hz': 30,
                   'codec': 'h264', 'bitrate_kbps': 4000,
                   'auto_rate': True, 'hdr_enabled': False}, f, indent=2)
        f.write('\n')
except FileExistsError:
    pass
