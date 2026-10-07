#!/usr/bin/env python3
"""Let a device with a working import reach the pump's drop-zerocopy fallback.

`PUNKTFUNK_CEDAR_REFUSE_IMPORT` makes the presenter refuse every imported dma-buf
frame, which is the exact signal the pump answers by asking the Cedar rung to drop
its hand-off (`Decoder::drop_zerocopy`). Without a hook the branch is unreachable
on this device: the import works, so nothing ever refuses, and the fallback ships
unexercised. The hook takes the same path the presenter already takes when it has
no import support - `force_software.store(true)` and `Ok(false)` - so the pump side
is not being tested through a private door.

Off unless the variable is set; read once.
"""
import pathlib
import sys

ARM_OLD = """            DecodedImage::NativeDmabuf(_) => {
                // No import extensions (or already demoted) - the pump rebuilds
"""
ARM_OLD = ARM_OLD.replace(' - the pump rebuilds', ' — the pump rebuilds')
ARM_NEW = """            DecodedImage::NativeDmabuf(_) => {
                // Test hook: refuse a frame the import path could have taken, so the
                // pump's drop-zerocopy fallback is reachable on a healthy device.
                if refuse_import() {
                    self.force_software.store(true, Ordering::Relaxed);
                    return Ok(false);
                }
                // No import extensions (or already demoted) — the pump rebuilds
"""

HELPER_OLD = "use super::*;\n\nimpl Shell {\n"
HELPER_NEW = """use super::*;

/// Test hook: `PUNKTFUNK_CEDAR_REFUSE_IMPORT` makes the presenter refuse every
/// imported dma-buf frame - the signal the pump's drop-zerocopy fallback answers.
/// Read once.
fn refuse_import() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| std::env::var_os("PUNKTFUNK_CEDAR_REFUSE_IMPORT").is_some())
}

impl Shell {
"""

EDITS = [("refuse hook in the dmabuf arm", ARM_OLD, ARM_NEW),
         ("the hook helper", HELPER_OLD, HELPER_NEW)]


def target_for(root: pathlib.Path) -> pathlib.Path:
    return root / 'crates/pf-presenter/src/run/pace.rs'


def main():
    root = pathlib.Path(sys.argv[1])
    path = target_for(root)
    text = path.read_text()
    for label, old, new in EDITS:
        if text.count(old) != 1:
            raise SystemExit(f'presenter refuse-import: {label} anchor drift; no writes')
        text = text.replace(old, new)
    path.write_text(text)
    print('presenter refuse-import: test hook installed')


if __name__ == '__main__':
    main()
