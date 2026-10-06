#!/usr/bin/env python3
"""Give TSPS Wi-Fi QUIC probes time to retry after UDP GSO fails."""
from pathlib import Path
import sys


def patch(root):
    edits = {
        'clients/session/src/console.rs': 'trust::probe_known(&hosts, Duration::from_millis(900))',
        'crates/pf-client-core/src/orchestrate.rs': 'crate::trust::probe_one(addr, port, fp_hex, Duration::from_millis(900))',
    }
    pending = []
    for name, old in edits.items():
        path = root / name
        text = path.read_text(encoding='utf-8')
        if text.count(old) != 1:
            raise SystemExit(f'unexpected upstream probe shape: {name}')
        new = old.replace('Duration::from_millis(900)', 'Duration::from_secs(3)')
        pending.append((path, text.replace(old, new)))
    for path, text in pending:
        path.write_text(text, encoding='utf-8')
    print('TSPS presence and wake probe budgets: 3 seconds (GSO fallback safe)')


if __name__ == '__main__':
    patch(Path(sys.argv[1]))
