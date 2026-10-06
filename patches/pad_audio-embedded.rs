//! Embedded PortMaster stub for PipeWire-only DualSense audio.
//!
//! Main gamepad input and rumble stay in the SDL gamepad path. SpruceOS has no
//! PipeWire graph for Punktfunk's controller speaker / voice-coil renderer, so
//! this build deliberately advertises none of those capabilities.

use punktfunk_core::client::NativeClient;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

pub fn speaker_active(_mode: &str) -> bool {
    false
}

pub(crate) fn is_tier_a_ds5(_vid: u16, _pid: u16) -> bool {
    false
}

pub(crate) fn wired_audio_sibling(_hid_path: Option<&str>) -> bool {
    false
}

pub(crate) fn register_tier_a(
    _index: u8,
    _hid_path: Option<String>,
    _bluetooth: bool,
) {
}

pub(crate) fn unregister_tier_a(_index: u8) {}

pub(crate) fn clear_haptics_liveness(_pad: u8) {}

pub(crate) fn haptics_live(_pad: u8) -> bool {
    false
}

pub(crate) fn note_rumble(_pad: u8, _low: u16, _high: u16, _ms: u32) {}

pub fn pad_audio_test(_seconds: u64, _coils: bool, _speaker: bool) -> anyhow::Result<()> {
    anyhow::bail!(
        "DualSense audio is unavailable in the SpruceOS embedded build (SDL rumble still works)"
    )
}

pub(crate) fn spawn(
    _connector: Arc<NativeClient>,
    _stop: Arc<AtomicBool>,
    _haptics: bool,
    _speaker: bool,
) -> Option<std::thread::JoinHandle<()>> {
    None
}
