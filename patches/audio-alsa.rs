//! Embedded ALSA playback backend for the PortMaster / SpruceOS build.
//!
//! Upstream Linux uses PipeWire. TrimUI Smart Pro S / SpruceOS exposes a
//! normal ALSA "default" PCM (prepared by spruce's asound-setup.sh), so this
//! backend keeps the Punktfunk protocol/decoder intact and only swaps the
//! device-facing audio sink. Microphone uplink and DualSense audio are
//! deliberately not implemented in this embedded build.

use anyhow::{anyhow, Context, Result};
use punktfunk_core::client::NativeClient;
use std::collections::VecDeque;
use std::ffi::{c_char, c_int, c_long, c_uint, c_ulong, c_void, CStr, CString};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, SyncSender, TryRecvError, TrySendError};
use std::sync::Arc;
use std::time::Duration;

const SND_PCM_STREAM_PLAYBACK: c_int = 0;
const SND_PCM_ACCESS_RW_INTERLEAVED: c_int = 3;
const SND_PCM_FORMAT_S16_LE: c_int = 2;
const ALSA_LATENCY_US: c_uint = 30_000;

#[link(name = "asound")]
unsafe extern "C" {
    fn snd_pcm_open(
        pcmp: *mut *mut c_void,
        name: *const c_char,
        stream: c_int,
        mode: c_int,
    ) -> c_int;
    fn snd_pcm_close(pcm: *mut c_void) -> c_int;
    fn snd_pcm_drop(pcm: *mut c_void) -> c_int;
    fn snd_pcm_set_params(
        pcm: *mut c_void,
        format: c_int,
        access: c_int,
        channels: c_uint,
        rate: c_uint,
        soft_resample: c_int,
        latency: c_uint,
    ) -> c_int;
    fn snd_pcm_writei(pcm: *mut c_void, buffer: *const c_void, size: c_ulong) -> c_long;
    fn snd_pcm_recover(pcm: *mut c_void, err: c_int, silent: c_int) -> c_int;
    fn snd_pcm_delay(pcm: *mut c_void, delayp: *mut c_long) -> c_int;
    fn snd_strerror(errnum: c_int) -> *const c_char;
}

fn alsa_error(code: c_int) -> String {
    // SAFETY: libasound returns a process-lifetime NUL-terminated string for an errno.
    let p = unsafe { snd_strerror(code) };
    if p.is_null() {
        return format!("ALSA error {code}");
    }
    // SAFETY: snd_strerror's contract guarantees p points to a valid C string.
    unsafe { CStr::from_ptr(p) }.to_string_lossy().into_owned()
}

struct Pcm(*mut c_void);

impl Pcm {
    fn open(fmt: PlaybackFormat) -> Result<Self> {
        let device = std::env::var("PUNKTFUNK_ALSA_DEVICE")
            .ok()
            .filter(|v| !v.is_empty())
            .unwrap_or_else(|| "default".to_string());
        let device_c = CString::new(device.as_bytes()).context("ALSA device contains NUL")?;
        let mut raw = std::ptr::null_mut();

        // SAFETY: raw is a valid out-pointer and device_c is NUL-terminated for this call.
        let rc = unsafe {
            snd_pcm_open(
                &mut raw,
                device_c.as_ptr(),
                SND_PCM_STREAM_PLAYBACK,
                0,
            )
        };
        if rc < 0 || raw.is_null() {
            return Err(anyhow!(
                "snd_pcm_open({device}): {}",
                alsa_error(if rc < 0 { rc } else { -1 })
            ));
        }

        let pcm = Self(raw);
        // S16_LE is intentional: the TSPS codec path is stereo/48 kHz and this
        // avoids requiring float-format support from the firmware's dmix/asym chain.
        // soft_resample=1 lets spruce's "default" plug route Bluetooth too.
        // SAFETY: pcm.0 is live until Pcm::drop and the scalar arguments follow ALSA's ABI.
        let rc = unsafe {
            snd_pcm_set_params(
                pcm.0,
                SND_PCM_FORMAT_S16_LE,
                SND_PCM_ACCESS_RW_INTERLEAVED,
                fmt.channels,
                fmt.rate_hz,
                1,
                ALSA_LATENCY_US,
            )
        };
        if rc < 0 {
            return Err(anyhow!("snd_pcm_set_params: {}", alsa_error(rc)));
        }

        tracing::info!(
            device = %device,
            channels = fmt.channels,
            rate_hz = fmt.rate_hz,
            latency_us = ALSA_LATENCY_US,
            "ALSA playback ready"
        );
        Ok(pcm)
    }

    fn delay_frames(&self) -> u64 {
        let mut delay: c_long = 0;
        // SAFETY: self.0 is a live ALSA PCM and delay is a valid out-pointer.
        let rc = unsafe { snd_pcm_delay(self.0, &mut delay) };
        if rc < 0 {
            0
        } else {
            delay.max(0) as u64
        }
    }

    fn write_frames(&self, data: &[i16], channels: usize) -> Result<()> {
        if channels == 0 || data.is_empty() {
            return Ok(());
        }
        let total_frames = data.len() / channels;
        let mut done = 0usize;

        while done < total_frames {
            let sample_off = done * channels;
            // SAFETY: sample_off stays inside data and ALSA reads at most the requested frames.
            let rc = unsafe {
                snd_pcm_writei(
                    self.0,
                    data[sample_off..].as_ptr().cast::<c_void>(),
                    (total_frames - done) as c_ulong,
                )
            };
            if rc >= 0 {
                done += rc as usize;
                continue;
            }

            // SAFETY: self.0 is live; snd_pcm_recover is the documented xrun/suspend recovery API.
            let recovered = unsafe { snd_pcm_recover(self.0, rc as c_int, 1) };
            if recovered < 0 {
                return Err(anyhow!(
                    "snd_pcm_writei: {}; recover: {}",
                    alsa_error(rc as c_int),
                    alsa_error(recovered)
                ));
            }
        }
        Ok(())
    }
}

impl Drop for Pcm {
    fn drop(&mut self) {
        if self.0.is_null() {
            return;
        }
        // SAFETY: the pointer is owned by this Pcm and closed exactly once here.
        unsafe {
            let _ = snd_pcm_drop(self.0);
            let _ = snd_pcm_close(self.0);
        }
    }
}

/// Device output exposed to the session's debug picker.
#[derive(Clone, Debug)]
pub struct AudioDevice {
    pub name: String,
    pub description: String,
}

pub fn devices() -> Result<(Vec<AudioDevice>, Vec<AudioDevice>)> {
    Ok((
        vec![AudioDevice {
            name: "default".to_string(),
            description: "SpruceOS default ALSA output".to_string(),
        }],
        Vec::new(),
    ))
}

#[derive(Clone, Copy, Debug)]
pub struct PlaybackFormat {
    pub channels: u32,
    pub rate_hz: u32,
    pub frame_us: u32,
}

impl PlaybackFormat {
    fn quantum_frames(&self) -> u32 {
        ((self.rate_hz as u64 * self.frame_us as u64 / 1_000_000) as u32).max(1)
    }
}

pub struct AudioPlayer {
    pcm_tx: SyncSender<Vec<f32>>,
    recycle_rx: Receiver<Vec<f32>>,
    quit: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
    sync: Arc<punktfunk_core::audio::AudioSyncCell>,
    vitals: Arc<crate::audio_vitals::PlaybackVitals>,
}

impl AudioPlayer {
    pub fn spawn(fmt: PlaybackFormat) -> Result<AudioPlayer> {
        if fmt.channels == 0 || fmt.channels > 8 {
            return Err(anyhow!("unsupported ALSA channel count {}", fmt.channels));
        }

        let (pcm_tx, pcm_rx) = std::sync::mpsc::sync_channel::<Vec<f32>>(64);
        let (recycle_tx, recycle_rx) = std::sync::mpsc::sync_channel::<Vec<f32>>(64);
        let (ready_tx, ready_rx) = std::sync::mpsc::sync_channel::<Result<(), String>>(1);
        let quit = Arc::new(AtomicBool::new(false));
        let sync: Arc<punktfunk_core::audio::AudioSyncCell> = Arc::default();
        let vitals: Arc<crate::audio_vitals::PlaybackVitals> = Arc::default();

        let quit_worker = quit.clone();
        let sync_worker = sync.clone();
        let vitals_worker = vitals.clone();
        let thread = std::thread::Builder::new()
            .name("punktfunk-audio-alsa".into())
            .spawn(move || {
                let pcm = match Pcm::open(fmt) {
                    Ok(pcm) => {
                        let _ = ready_tx.send(Ok(()));
                        pcm
                    }
                    Err(e) => {
                        let msg = format!("{e:#}");
                        let _ = ready_tx.send(Err(msg.clone()));
                        tracing::warn!(error = %msg, "ALSA playback unavailable");
                        return;
                    }
                };
                if let Err(e) = playback_loop(
                    pcm,
                    pcm_rx,
                    recycle_tx,
                    fmt,
                    quit_worker,
                    sync_worker,
                    vitals_worker,
                ) {
                    tracing::warn!(error = %format!("{e:#}"), "ALSA playback thread ended");
                }
            })
            .context("spawn ALSA playback thread")?;

        match ready_rx.recv_timeout(Duration::from_secs(3)) {
            Ok(Ok(())) => {}
            Ok(Err(msg)) => {
                let _ = thread.join();
                return Err(anyhow!(msg));
            }
            Err(e) => {
                quit.store(true, Ordering::SeqCst);
                let _ = thread.join();
                return Err(anyhow!("ALSA playback startup timeout: {e}"));
            }
        }

        Ok(AudioPlayer {
            pcm_tx,
            recycle_rx,
            quit,
            thread: Some(thread),
            sync,
            vitals,
        })
    }

    pub fn sync_cell(&self) -> Arc<punktfunk_core::audio::AudioSyncCell> {
        self.sync.clone()
    }

    pub fn vitals(&self) -> Arc<crate::audio_vitals::PlaybackVitals> {
        self.vitals.clone()
    }

    pub fn take_buffer(&self) -> Vec<f32> {
        self.recycle_rx.try_recv().unwrap_or_default()
    }

    pub fn push(&self, pcm: Vec<f32>) {
        if let Err(TrySendError::Disconnected(_)) = self.pcm_tx.try_send(pcm) {
            // Playback thread already ended.
        }
    }
}

impl Drop for AudioPlayer {
    fn drop(&mut self) {
        self.quit.store(true, Ordering::SeqCst);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

pub(crate) const TUNING: punktfunk_core::audio::JitterTuning =
    punktfunk_core::audio::JitterTuning::WASAPI;

pub fn can_render_at(rate_hz: u32) -> bool {
    rate_hz <= 48_000
}

fn playback_loop(
    pcm: Pcm,
    rx: Receiver<Vec<f32>>,
    recycle: SyncSender<Vec<f32>>,
    fmt: PlaybackFormat,
    quit: Arc<AtomicBool>,
    sync: Arc<punktfunk_core::audio::AudioSyncCell>,
    vitals: Arc<crate::audio_vitals::PlaybackVitals>,
) -> Result<()> {
    crate::audio_rt::boost_and_log("punktfunk-audio-alsa");

    let channels = fmt.channels as usize;
    let want_frames = fmt.quantum_frames() as usize;
    let want = want_frames * channels;
    let per_ms = fmt.rate_hz as usize * channels / 1000;
    let mut ring = VecDeque::<f32>::with_capacity(
        per_ms * TUNING.hard_cap_ms as usize + 64 * want,
    );
    let mut policy = punktfunk_core::audio::JitterPolicy::new_at_rate(
        TUNING,
        fmt.channels as u8,
        fmt.rate_hz,
    );
    policy.set_frame_us(fmt.frame_us);

    let tick = Duration::from_micros(u64::from(fmt.frame_us).max(1));
    let mut out_f32 = vec![0.0f32; want];
    let mut out_s16 = vec![0i16; want];

    if !vitals.quantum_known() {
        vitals.note_quantum(want_frames as u32, want_frames as u32, want_frames as u32);
    }

    while !quit.load(Ordering::SeqCst) {
        match rx.recv_timeout(tick) {
            Ok(mut chunk) => {
                ring.extend(chunk.iter().copied());
                chunk.clear();
                let _ = recycle.try_send(chunk);
            }
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
        }
        loop {
            match rx.try_recv() {
                Ok(mut chunk) => {
                    ring.extend(chunk.iter().copied());
                    chunk.clear();
                    let _ = recycle.try_send(chunk);
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => break,
            }
        }

        policy.set_sync_target(sync.target());
        sync.publish_depth(ring.len());
        let delay_frames = pcm.delay_frames();
        let delay_ns = delay_frames.saturating_mul(1_000_000_000) / u64::from(fmt.rate_hz);
        sync.publish_output_latency_ns(delay_ns);

        let step = policy.step(ring.len(), want);
        if step.drop_front > 0 {
            punktfunk_core::audio::crossfade_drop(&mut ring, step.drop_front, step.crossfade);
        }
        if step.insert_front > 0 {
            punktfunk_core::audio::crossfade_insert(&mut ring, step.insert_front, step.crossfade);
        }

        let mut ran_short = false;
        for dst in &mut out_f32 {
            *dst = if step.silence {
                0.0
            } else {
                ring.pop_front().unwrap_or_else(|| {
                    ran_short = true;
                    0.0
                })
            };
        }
        policy.note_read(ran_short);

        for (dst, src) in out_s16.iter_mut().zip(&out_f32) {
            *dst = (src.clamp(-1.0, 1.0) * 32767.0) as i16;
        }

        pcm.write_frames(&out_s16, channels)?;
        vitals.note_callback(
            ran_short,
            step.drop_front > 0,
            step.insert_front > 0,
            policy.avg_depth_ms(),
            policy.target_ms(),
        );
    }

    Ok(())
}

/// SpruceOS handheld builds do not expose a supported capture path to Punktfunk yet.
pub struct MicStreamer;

impl MicStreamer {
    pub fn spawn(
        _connector: Arc<NativeClient>,
        _muted: Arc<AtomicBool>,
        _echo_cancel: bool,
    ) -> Result<MicStreamer> {
        Err(anyhow!(
            "microphone uplink is disabled in the SpruceOS embedded audio backend"
        ))
    }
}
