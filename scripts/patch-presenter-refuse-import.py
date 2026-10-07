#!/usr/bin/env python3
"""Let a device with a working import reach the pump's drop-zerocopy fallback.

`PUNKTFUNK_CEDAR_REFUSE_IMPORT` makes the presenter refuse every imported dma-buf
frame, which is the exact signal the pump answers by asking the Cedar rung to drop
its hand-off (`Decoder::drop_zerocopy`). Without a hook the branch is unreachable
on this device: the import works, so nothing ever refuses, and the fallback ships
unexercised. The hook takes the path the presenter already takes when it has no
import support - `force_software.store(true)` and `Ok(false)` - so the pump side is
not being tested through a private door.

Both dmabuf arms get the hook, and the order matters: the guarded arm that presents
(`present_native`) is the one this device actually takes, and the unguarded arm is
reached only when there is no import support at all. A hook in the second arm alone
installs cleanly, looks right, and never fires.

Off unless the variable is set; read once. It says so in the log.
"""
import pathlib
import sys

HOOK = '''                if refuse_import() {
                    tracing::warn!(
                        "PUNKTFUNK_CEDAR_REFUSE_IMPORT: refusing an imported dma-buf frame"
                    );
                    self.force_software.store(true, Ordering::Relaxed);
                    return Ok(false);
                }
'''

GUARD_OLD = '''            DecodedImage::NativeDmabuf(d)
                if presenter.supports_dmabuf() && !self.health.demoted =>
            {
                self.hdr = d.color.is_pq();
'''
GUARD_NEW = '''            DecodedImage::NativeDmabuf(d)
                if presenter.supports_dmabuf() && !self.health.demoted =>
            {
''' + HOOK + '''                self.hdr = d.color.is_pq();
'''

ARM_OLD = '''            DecodedImage::NativeDmabuf(_) => {
                // No import extensions (or already demoted) \u2014 the pump rebuilds
'''
ARM_NEW = '''            DecodedImage::NativeDmabuf(_) => {
                // Test hook: refuse a frame the import path could have taken, so the
                // pump's drop-zerocopy fallback is reachable on a healthy device.
''' + HOOK + '''                // No import extensions (or already demoted) \u2014 the pump rebuilds
'''

HELPER_OLD = "use super::*;\n\nimpl Shell {\n"
HELPER_NEW = '''use super::*;

/// Test hook: `PUNKTFUNK_CEDAR_REFUSE_IMPORT` makes the presenter refuse every
/// imported dma-buf frame - the signal the pump's drop-zerocopy fallback answers.
/// Read once.
fn refuse_import() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| std::env::var_os("PUNKTFUNK_CEDAR_REFUSE_IMPORT").is_some())
}

impl Shell {
'''

EDITS = [("refuse hook in the presenting dmabuf arm", GUARD_OLD, GUARD_NEW),
         ("refuse hook in the no-import arm", ARM_OLD, ARM_NEW),
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
