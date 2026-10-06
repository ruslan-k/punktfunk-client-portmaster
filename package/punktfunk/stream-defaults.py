#!/usr/bin/env python3
"""First-run stream defaults for TSPS, keyed to the decode path in use.

60 FPS is device-verified on the native Cedar hardware rung (about 5 ms
received-to-pixels). A software fallback cannot hold that cadence, so it keeps
the measured 30 FPS budget. Existing settings, host presets and explicit UI
choices remain authoritative: this writes only when the file does not exist.
"""
import json
import os
from pathlib import Path
import sys

p = Path(sys.argv[1])
p.parent.mkdir(parents=True, exist_ok=True)
hardware = os.environ.get('PUNKTFUNK_DECODER', 'native-cedar') == 'native-cedar'
try:
    with p.open('x') as f:
        json.dump({'width': 1280, 'height': 720, 'refresh_hz': 60 if hardware else 30,
                   'codec': 'h264', 'bitrate_kbps': 4000,
                   'auto_rate': True, 'hdr_enabled': False}, f, indent=2)
        f.write('\n')
except FileExistsError:
    pass
