//! Native Allwinner Cedar decode for A523-class handhelds (TSPS / SpruceOS).
//!
//! The vendor BSP ships an H.264 hardware decoder behind `libvdecoder.so`
//! (CedarX v2 "vdecoder" API). This rung `dlopen`s that stack at session
//! start — never links it, so the package keeps building and shipping with no
//! vendor library present — registers its plugin, and copies decoded pictures
//! into the same CPU planar path the software rung feeds
//! ([`CpuPlanarFrame`]).
//!
//! Scope, deliberately: H.264 only, pin-only
//! (`PUNKTFUNK_DECODER=native-cedar`), 8-bit 4:2:0 only, and no dmabuf
//! hand-off yet. The picture copy is ~1.4 MB a frame at 720p — nothing
//! against the decoder's own throughput — and it keeps this first hardware
//! rung on the frame path that is already proven end to end, so timing, frame
//! order and colour stay comparable with software frame for frame.
//! `VideoPicture::n_buf_fd` is already carried by the ABI transcription for a
//! later zero-copy presenter import.
//!
//! Every vendor call goes through the ABI the public H6-CedarC `vdecoder.h`
//! declares; the transcriptions below are pinned by unit tests (`size_of` /
//! `offset_of`) against those headers, and the plane layouts (planar chroma
//! stride `lineStride/2`, `YUV_PLANER_420` = Y,U,V, `YV12` = Y,V,U, `NV12` =
//! U,V interleaved, `NV21` = V,U) follow the vendor `pixel_format.c`
//! conversions. Failures are ordinary `Err`s: the session's demotion streak
//! then hands the stream to software, as with every other native rung.
//! Progress and every counter log under the `cedar` target.

use std::collections::VecDeque;
use std::ffi::c_char;
use std::ffi::c_int;
use std::ffi::c_uint;
use std::ffi::c_void;
use std::path::Path;
use std::time::Instant;

#[path = "cedar_phases.rs"]
mod phases;
use phases::{PhaseStats, elapsed_us, queue_picture};
#[path = "cedar_pts.rs"]
mod pts;
use pts::{PtsLedger, TokenClock};
pub(crate) use pts::FrameStamp;
#[path = "cedar_tuning.rs"]
mod tuning;
use tuning::CedarTuning;
#[path = "cedar_async.rs"]
mod async_parser;
#[path = "cedar_low_delay.rs"]
mod low_delay;

use anyhow::anyhow;
use anyhow::bail;
use anyhow::ensure;
use anyhow::Context as _;
use anyhow::Result;
use pf_bitstream::h264::H264Planner;
use pf_bitstream::h264::PlanError;
use pf_vkdecode::RecoveryWatch;

use crate::video::CpuPlanarFrame;
use crate::video::StreamFormat;
use crate::video_color::ColorDesc;
use crate::video_software::NoSoftwareRung;

/// `PUNKTFUNK_DECODER=native-cedar`. Pin-only: `auto` never reaches this rung.
pub(crate) const DECODER_PIN: &str = "native-cedar";

/// The vendor stream/picture-buffer index for the single stream this rung opens.
const STREAM_INDEX: c_int = 0;

/// `VideoStreamDataInfo` caps its own length at `int`; refuse longer AUs
/// instead of wrapping.
const MAX_AU_BYTES: usize = 16 * 1024 * 1024;

/// Decode calls drained per fed AU. One-in/one-out hosts produce one frame
/// per AU; the cap only bounds a wedged vendor loop.
const DRAIN_ROUNDS: usize = 64;

/// One `cedar:` cadence line per this many decoded pictures.
const LOG_EVERY_FRAMES: u64 = 300;

/// `VIDEO_CODEC_FORMAT_H264` (H6-CedarC `vdecoder.h`).
const VIDEO_CODEC_FORMAT_H264: c_int = 0x115;
/// `PIXEL_FORMAT_YUV_PLANER_420`: plane order Y, U, V.
const PIXEL_FORMAT_YUV_PLANER_420: c_int = 1;
/// `PIXEL_FORMAT_YV12`: plane order Y, V, U — chroma swapped against I420.
const PIXEL_FORMAT_YV12: c_int = 4;
/// `PIXEL_FORMAT_NV21`: Y, then interleaved V, U.
const PIXEL_FORMAT_NV21: c_int = 5;
/// `PIXEL_FORMAT_NV12`: Y, then interleaved U, V.
const PIXEL_FORMAT_NV12: c_int = 6;

/// `EVDECODERESULT` values the drain loop acts on.
const VDECODE_RESULT_OK: c_int = 0;
const VDECODE_RESULT_FRAME_DECODED: c_int = 1;
const VDECODE_RESULT_CONTINUE: c_int = 2;
const VDECODE_RESULT_KEYFRAME_DECODED: c_int = 3;
const VDECODE_RESULT_NO_FRAME_BUFFER: c_int = 4;
const VDECODE_RESULT_NO_BITSTREAM: c_int = 5;
const VDECODE_RESULT_RESOLUTION_CHANGE: c_int = 6;

/// Vendor libraries, in load order. `RTLD_GLOBAL` insertions are the point:
/// `AddVDPlugin` and the decoder's own cross-library calls resolve against
/// them. The plugin host (`libvdecoder`) loads last and `libawh264.so`
/// registers from inside it. `required` = the stack cannot work without it.
const VENDOR_LIBS: [(&str, bool); 6] = [
    ("/usr/lib/libcdc_base.so", false),
    ("/usr/lib/libMemAdapter.so", true),
    ("/usr/lib/libVE.so", false),
    ("/usr/lib/libvideoengine.so", false),
    ("/usr/lib/libcedare.so", false),
    ("/usr/lib/libvdecoder.so", true),
];

// ---------------------------------------------------------------------------
// Vendor ABI (H6-CedarC `vdecoder.h` / `typedef.h`, aarch64 LP64).
// ---------------------------------------------------------------------------

/// `VideoStreamInfo`. Field order and widths are the header's; `size_of` and
/// `offset_of` tests pin them.
#[repr(C)]
#[allow(dead_code)] // Read by the vendor, written here — the whole struct is the contract.
struct VideoStreamInfo {
    e_codec_format: c_int,
    n_width: c_int,
    n_height: c_int,
    n_frame_rate: c_int,
    n_frame_duration: c_int,
    n_aspect_ratio: c_int,
    b_is_3d_stream: c_int,
    n_codec_specific_data_len: c_int,
    p_codec_specific_data: *mut c_char,
    b_secure_stream_flag: c_int,
    b_secure_stream_flag_level1: c_int,
    b_is_frame_package: c_int,
    h265_reference_picture_num: c_int,
    b_re_open_engine: c_int,
    b_is_frame_cts_test_flag: c_int,
}

/// `VConfig`. The device memcpys its own `sizeof(VConfig)` from our pointer,
/// so callers pass [`VConfigBuf`] (this plus a zeroed tail) and never a bare
/// `VideoConfig`.
#[repr(C)]
#[allow(dead_code)]
struct VideoConfig {
    b_scale_down_en: c_int,
    b_rotation_en: c_int,
    b_sec_output_en: c_int,
    n_horizon_scale_down_ratio: c_int,
    n_vertical_scale_down_ratio: c_int,
    n_sec_horizon_scale_down_ratio: c_int,
    n_sec_vertical_scale_down_ratio: c_int,
    n_rotate_degree: c_int,
    b_thumbnail_mode: c_int,
    e_output_pixel_format: c_int,
    e_sec_output_pixel_format: c_int,
    b_no_b_frames: c_int,
    b_disable_3d: c_int,
    b_support_maf: c_int,
    b_disp_error_frame: c_int,
    n_vbv_buffer_size: c_int,
    n_frame_buffer_num: c_int,
    b_secureos_en: c_int,
    b_gpu_buf_valid: c_int,
    // The A523 device's VideoConfig carries three more fields here, between
    // bGpuBufValid and nAlignStride — absent from the H6-CedarC
    // transcription this rung started from. Evidence: the device's own
    // vdecoderDemo reads its holding counts at 0x68..0x74, memops at 0x80
    // and nVeFreq at 0xA4 (sizeof 216); and with the H6 offsets our write
    // that should have been palloc landed where the device reads
    // nAlignStride (its FBM create line printed "nAlignStride = 1" while
    // the working demo prints 0). Names unknown; values stay zeroed.
    reserved_after_gpu_valid_0: c_int,
    reserved_after_gpu_valid_1: c_int,
    reserved_after_gpu_valid_2: c_int,
    n_align_stride: c_int,
    b_is_soft_decoder_flag: c_int,
    b_vir_malloc_sbm: c_int,
    b_support_palloc_buf_before_decode: c_int,
    n_de_interlace_holding_frame_buffer_num: c_int,
    n_display_holding_frame_buffer_num: c_int,
    n_rotate_holding_frame_buffer_num: c_int,
    n_decode_smooth_frame_buffer_num: c_int,
    b_is_tv_stream: c_int,
    memops: *mut c_void,
    e_ctl_afbc_mode: c_int,
    e_ctl_iptv_mode: c_int,
    ve_ops_s: *mut c_void,
    p_ve_ops_self: *mut c_void,
    b_convert_vp910bit_to_8bit: c_int,
    n_ve_freq: c_uint,
    b_called_by_omx_flag: c_int,
    b_set_proc_info_enable: c_int,
    n_set_proc_info_freq: c_int,
    n_channel_num: c_int,
    /// The device copies 216 bytes. Apart from the guarded output-gate word
    /// at +192, the SDK-unknown tail remains zeroed and untouched.
    _tail_before_common_flags: [u8; 8],
    /// Observed libawh264 output-hold gate; SDK name unknown. Version guarded.
    common_config_flags_192: c_uint,
    _tail: [u8; 20],
}

/// Zeroed tail for the device's `memcpy(p->vconfig, pVconfig, sizeof(VConfig))`:
/// keeps that read inside our allocation even if the device's struct has grown
/// fields this (H6-CedarC) transcription predates.
const VCONFIG_TAIL_BYTES: usize = 256;

#[repr(C)]
struct VConfigBuf {
    config: VideoConfig,
    tail: [u8; VCONFIG_TAIL_BYTES],
}

/// `VideoStreamDataInfo`.
#[repr(C)]
#[allow(dead_code)]
struct VideoStreamDataInfo {
    p_data: *mut c_char,
    n_length: c_int,
    n_pts: i64,
    n_pcr: i64,
    b_is_first_part: c_int,
    b_is_last_part: c_int,
    n_id: c_int,
    n_stream_index: c_int,
    b_valid: c_int,
    b_video_info_flag: c_uint,
    p_video_info: *mut c_void,
}

/// `VIDEO_FRM_MV_INFO`: ten `s16`s.
#[repr(C)]
#[allow(dead_code)]
struct VideoFrmMvInfo {
    n_max_mv_x: i16,
    n_min_mv_x: i16,
    n_avg_mv_x: i16,
    n_max_mv_y: i16,
    n_min_mv_y: i16,
    n_avg_mv_y: i16,
    n_max_mv: i16,
    n_min_mv: i16,
    n_avg_mv: i16,
    skip_ratio: i16,
}

/// `VIDEO_FRM_STATUS_INFO`, the tail of [`VideoPicture`].
#[repr(C)]
#[allow(dead_code)]
struct VideoFrmStatusInfo {
    en_vid_frm_type: c_int,
    n_vid_frm_size: c_int,
    n_vid_frm_dis_w: c_int,
    n_vid_frm_dis_h: c_int,
    n_vid_frm_qp: c_int,
    n_aver_bit_rate: f64,
    n_frame_rate: f64,
    n_vid_frm_pts: i64,
    n_mv_info: VideoFrmMvInfo,
    b_drop_pre_frame: c_int,
}

/// `VideoPicture`. The whole struct is transcribed so field offsets stay
/// honest; this rung reads the picture geometry, the plane pointers, the
/// pixel format and (for the future dmabuf milestone) `n_buf_fd`.
#[repr(C)]
#[allow(dead_code)]
struct VideoPicture {
    n_id: c_int,
    n_stream_index: c_int,
    e_pixel_format: c_int,
    n_width: c_int,
    n_height: c_int,
    n_line_stride: c_int,
    n_top_offset: c_int,
    n_left_offset: c_int,
    n_bottom_offset: c_int,
    n_right_offset: c_int,
    n_frame_rate: c_int,
    n_aspect_ratio: c_int,
    b_is_progressive: c_int,
    b_top_field_first: c_int,
    b_repeat_top_field: c_int,
    n_pts: i64,
    n_pcr: i64,
    p_data0: *mut c_char,
    p_data1: *mut c_char,
    p_data2: *mut c_char,
    p_data3: *mut c_char,
    b_maf_valid: c_int,
    p_maf_data: *mut c_char,
    n_maf_flag_stride: c_int,
    b_pre_frm_valid: c_int,
    n_buf_id: c_int,
    phy_y_buf_addr: usize,
    phy_c_buf_addr: usize,
    p_private: *mut c_void,
    n_buf_fd: c_int,
    n_buf_status: c_int,
    b_top_field_error: c_int,
    b_bottom_field_error: c_int,
    n_color_primary: c_int,
    b_frame_error_flag: c_int,
    p_meta_data: *mut c_void,
    video_full_range_flag: c_int,
    transfer_characteristics: c_int,
    matrix_coeffs: c_int,
    colour_primaries: u8,
    n_lower2bit_buf_size: c_int,
    n_lower2bit_buf_offset: c_int,
    n_lower2bit_buf_stride: c_int,
    b10bit_pic_flag: c_int,
    b_enable_afbc_flag: c_int,
    n_buf_size: c_int,
    n_afbc_size: c_int,
    n_debug_count: c_int,
    n_cur_frame_info: VideoFrmStatusInfo,
}

type AddVdPluginFn = unsafe extern "C" fn();
type CreateDecoderFn = unsafe extern "C" fn() -> *mut c_void;
type DestroyDecoderFn = unsafe extern "C" fn(*mut c_void);
type InitializeDecoderFn =
    unsafe extern "C" fn(*mut c_void, *mut VideoStreamInfo, *mut VideoConfig) -> c_int;
type DecodeStreamFn = unsafe extern "C" fn(*mut c_void, c_int, c_int, c_int, i64) -> c_int;
type RequestStreamBufferFn = unsafe extern "C" fn(
    *mut c_void,
    c_int,
    *mut *mut c_char,
    *mut c_int,
    *mut *mut c_char,
    *mut c_int,
    c_int,
) -> c_int;
type SubmitStreamDataFn =
    unsafe extern "C" fn(*mut c_void, *mut VideoStreamDataInfo, c_int) -> c_int;
type RequestPictureFn = unsafe extern "C" fn(*mut c_void, c_int) -> *mut VideoPicture;
type ReturnPictureFn = unsafe extern "C" fn(*mut c_void, *mut VideoPicture) -> c_int;

/// Handles plus the resolved entry points. One per session; dropping it closes
/// the libraries once the decoder handle is destroyed.
struct CedarLibs {
    /// Kept for ownership (drop closes); never read after `load`.
    _handles: Vec<libloading::Library>,
    /// `libvdecoder` itself, kept alive beside every symbol resolved from it.
    _vdecoder: libloading::Library,
    create: CreateDecoderFn,
    destroy: DestroyDecoderFn,
    init: InitializeDecoderFn,
    decode: DecodeStreamFn,
    request_stream_buffer: RequestStreamBufferFn,
    submit_stream_data: SubmitStreamDataFn,
    request_picture: RequestPictureFn,
    return_picture: ReturnPictureFn,
}

impl CedarLibs {
    /// Load the vendor stack and resolve the entry points this rung calls.
    /// Missing optional libraries log and continue; a missing required one —
    /// or a missing symbol — refuses the rung, and the pin falls back to the
    /// standard ladder.
    fn load() -> Result<CedarLibs> {
        let mut handles: Vec<libloading::Library> = Vec::new();
        for (path, required) in VENDOR_LIBS {
            if !Path::new(path).exists() {
                if required {
                    bail!("cedar: {path} is missing — the vendor decoder stack is not installed");
                }
                tracing::info!(target: "cedar", path, "cedar: optional vendor library absent — skipped");
                continue;
            }
            // SAFETY: opening one fixed, root-owned vendor library with
            // RTLD_NOW|RTLD_GLOBAL. Its initialisers are the vendor's and run
            // on load; the handle is owned here and outlives every symbol
            // resolved from it.
            let lib = unsafe {
                libloading::os::unix::Library::open(Some(path), libc::RTLD_NOW | libc::RTLD_GLOBAL)
            }
            .with_context(|| format!("cedar: dlopen {path}"))?;
            handles.push(libloading::Library::from(lib));
        }
        let vdecoder = handles
            .pop()
            .context("cedar: libvdecoder was not in the load set")?;

        // SAFETY: each `get` resolves one symbol from the live libvdecoder
        // handle kept beside these pointers; the types are the vendor
        // header's own declarations, so calls through them are its contract.
        // `AddVDPlugin` runs before the decoder is created — the vendor stack
        // reports H.264 as recognized but refuses it as unsupported when the
        // plugin was not registered first.
        let libs = unsafe {
            let add_vd_plugin = *vdecoder
                .get::<AddVdPluginFn>(b"AddVDPlugin\0")
                .context("cedar: dlsym AddVDPlugin")?;
            add_vd_plugin();
            let create = *vdecoder
                .get::<CreateDecoderFn>(b"CreateVideoDecoder\0")
                .context("cedar: dlsym CreateVideoDecoder")?;
            let destroy = *vdecoder
                .get::<DestroyDecoderFn>(b"DestroyVideoDecoder\0")
                .context("cedar: dlsym DestroyVideoDecoder")?;
            let init = *vdecoder
                .get::<InitializeDecoderFn>(b"InitializeVideoDecoder\0")
                .context("cedar: dlsym InitializeVideoDecoder")?;
            let decode = *vdecoder
                .get::<DecodeStreamFn>(b"DecodeVideoStream\0")
                .context("cedar: dlsym DecodeVideoStream")?;
            let request_stream_buffer = *vdecoder
                .get::<RequestStreamBufferFn>(b"RequestVideoStreamBuffer\0")
                .context("cedar: dlsym RequestVideoStreamBuffer")?;
            let submit_stream_data = *vdecoder
                .get::<SubmitStreamDataFn>(b"SubmitVideoStreamData\0")
                .context("cedar: dlsym SubmitVideoStreamData")?;
            let request_picture = *vdecoder
                .get::<RequestPictureFn>(b"RequestPicture\0")
                .context("cedar: dlsym RequestPicture")?;
            let return_picture = *vdecoder
                .get::<ReturnPictureFn>(b"ReturnPicture\0")
                .context("cedar: dlsym ReturnPicture")?;
            CedarLibs {
                _handles: handles,
                _vdecoder: vdecoder,
                create,
                destroy,
                init,
                decode,
                request_stream_buffer,
                submit_stream_data,
                request_picture,
                return_picture,
            }
        };
        tracing::info!(target: "cedar", "cedar: vendor libvdecoder stack loaded and plugin registered");
        Ok(libs)
    }
}

/// Planner facts for one AU, mirroring the software rung's fold so colour and
/// recovery behave identically across the two decoders.
#[derive(Debug, Clone, Copy)]
struct CedarFacts {
    is_idr: bool,
    /// `None` = AU did not plan; the last colour stays.
    color: Option<ColorDesc>,
    recovery: punktfunk_core::reanchor::LocalRecovery,
}

#[derive(Clone, Copy)]
struct SubmittedPicture {
    source_au: u64,
    stamp: Option<FrameStamp>,
    facts: CedarFacts,
    color: ColorDesc,
}
struct CedarOutput {
    frame: CpuPlanarFrame,
    stamp: Option<FrameStamp>,
}

pub(crate) struct NativeCedarDecoder {
    libs: CedarLibs,
    /// `VideoDecoder*` from `CreateVideoDecoder`; NULL only before check.
    handle: *mut c_void,
    /// Coded dimensions from the first plannable AU; `InitializeVideoDecoder`
    /// needs real geometry, and the SPS is where it comes from.
    pending_dims: Option<(u32, u32)>,
    /// Set once `InitializeVideoDecoder` returned 0.
    inited: bool,
    planner: Box<H264Planner>,
    recovery: RecoveryWatch,
    /// Last colour the stream stated; seed is the SDR default, as software.
    color: ColorDesc,
    /// No AUs are fed before the first IDR: pre-anchor frames cannot predict.
    anchored: bool,
    /// Frames decoded ahead of the one-in/one-out ask (an SBM-full drain can
    /// out-produce a single AU). Returned front-first so display order equals
    /// decode order; bounded by construction to a couple of frames.
    pending: VecDeque<CedarOutput>,
    plan_warned: bool,
    offsets_logged: bool,
    format_logged: bool,
    acked_anchor_message: bool,
    // Counters for the `cedar:` cadence line.
    aus: u64,
    frames_out: u64,
    empties: u64,
    errors: u64,
    logged_at: u64,
    profile: Option<PhaseStats>,
    output_fifo: bool,
    immediate_handoff: bool,
    drop_b_delay: c_int,
    poll_budget_us: u64,
    append_aud: bool,
    low_delay: bool,
    zero_reorder_verified: bool,
    output_lag_frames: [u64; 5],
    pts_probe: bool,
    pts_ledger: PtsLedger<SubmittedPicture>,
    pts_clock: TokenClock,
    input_stamp: Option<FrameStamp>,
    output_stamp: Option<FrameStamp>,
    pts_matches: u64,
    pts_unmatched: u64,
    start: Instant,
}

// SAFETY: the vendor decoder context is single-threaded by contract and this
// type owns it exclusively (no `Clone`, no `Sync`); it lives on the session
// pump thread with every other backend's device handles.
unsafe impl Send for NativeCedarDecoder {}

impl NativeCedarDecoder {
    pub(crate) fn name(&self) -> &'static str {
        "cedar (vendor libvdecoder)"
    }

    pub(crate) fn new(wire: u8, stream: StreamFormat) -> Result<NativeCedarDecoder> {
        if wire != punktfunk_core::quic::CODEC_H264 {
            bail!(
                "cedar is H.264-only in this build (session negotiated {})",
                crate::video::wire_codec_name(wire)
            );
        }
        if stream.bit_depth != 8 || stream.chroma_format_idc != punktfunk_core::quic::CHROMA_IDC_420
        {
            bail!(
                "cedar accepts 8-bit 4:2:0 only (stream: {}-bit, chroma_format_idc {})",
                stream.bit_depth,
                stream.chroma_format_idc
            );
        }
        let libs = CedarLibs::load()?;
        // SAFETY: `create` is the header's `VideoDecoder* CreateVideoDecoder(void)`
        // from the live libvdecoder handle; it returns an owned context or NULL.
        let handle = unsafe { (libs.create)() };
        if handle.is_null() {
            bail!("cedar: CreateVideoDecoder returned NULL");
        }
        tracing::info!(target: "cedar", "cedar: decoder context created (H.264, initialize deferred to the first SPS)");
        Ok(NativeCedarDecoder {
            libs,
            handle,
            pending_dims: None,
            inited: false,
            planner: Box::new(H264Planner::new()),
            recovery: RecoveryWatch::new(),
            color: ColorDesc {
                primaries: 2,
                transfer: 2,
                matrix: 2,
                full_range: false,
            },
            anchored: false,
            pending: VecDeque::new(),
            plan_warned: false,
            offsets_logged: false,
            format_logged: false,
            acked_anchor_message: false,
            aus: 0,
            frames_out: 0,
            empties: 0,
            errors: 0,
            logged_at: 0,
            profile: (std::env::var("PUNKTFUNK_CEDAR_PROFILE").as_deref() == Ok("1"))
                .then(PhaseStats::default),
            // Device A/B proved newest-wins discarded half the pictures.
            // Default FIFO; explicit 0 retains the old control for diagnostics.
            output_fifo: std::env::var("PUNKTFUNK_CEDAR_FIFO").as_deref() != Ok("0"),
            // Burst delivery alone halves presentation on newest-wins consumers.
            // Keep opt-in until the vendor's output cadence is one picture per AU.
            immediate_handoff: std::env::var("PUNKTFUNK_CEDAR_HANDOFF").as_deref() == Ok("1"),
            drop_b_delay: 0,
            poll_budget_us: 0,
            append_aud: false,
            low_delay: false,
            zero_reorder_verified: false,
            output_lag_frames: [0; 5],
            pts_probe: std::env::var("PUNKTFUNK_CEDAR_PTS_PROBE").as_deref() == Ok("1"),
            pts_ledger: PtsLedger::default(),
            pts_clock: TokenClock::default(),
            input_stamp: None,
            output_stamp: None,
            pts_matches: 0,
            pts_unmatched: 0,
            start: Instant::now(),
        })
    }

    /// The native-rung recovery channel. Cedar concealment surfaces as `Err`s
    /// (which already request a keyframe through the demotion streak), so this
    /// rung has nothing extra to report.
    pub(crate) fn take_recovery_request(&mut self) -> bool {
        false
    }

    pub(crate) fn set_input_stamp(&mut self, stamp: FrameStamp) {
        self.input_stamp = Some(stamp);
    }
    pub(crate) fn take_output_stamp(&mut self) -> Option<FrameStamp> {
        self.output_stamp.take()
    }

    /// Already copied pictures only: no AU feed, vendor call, wait, or metadata guess.
    pub(crate) fn poll_ready(&mut self) -> Option<CpuPlanarFrame> {
        if !self.immediate_handoff { return None; }
        let output = phases::poll_picture(&mut self.pending)?;
        self.frames_out += 1;
        self.output_stamp = output.stamp;
        Some(output.frame)
    }

    pub(crate) fn decode(&mut self, au: &[u8]) -> Result<Option<CpuPlanarFrame>> {
        self.output_stamp = None;
        let begin = self.profile.as_ref().map(|_| Instant::now());
        let result = self.decode_inner(au);
        if let Some(profile) = self.profile.as_mut() {
            profile.note_stage(2, elapsed_us(begin));
            if self.aus.is_multiple_of(120) {
                tracing::info!(target: "cedar", summary = %profile.json(self.aus, self.frames_out),
                    "cedar-phase-json");
                *profile = PhaseStats::default();
            }
        }
        if let Err(e) = &result {
            self.errors += 1;
            tracing::warn!(target: "cedar", error = %format!("{e:#}"),
                "cedar: decode error (the demotion streak decides whether software takes over)");
        }
        result
    }

    fn decode_inner(&mut self, au: &[u8]) -> Result<Option<CpuPlanarFrame>> {
        self.aus += 1;
        let begin = self.profile.as_ref().map(|_| Instant::now());
        let facts = self.plan_facts(au)?;
        if let Some(profile) = self.profile.as_mut() {
            profile.note_stage(0, elapsed_us(begin));
        }
        if let Some(c) = facts.color {
            self.color = c;
        }
        // Before the first IDR there is nothing to predict from; skipping
        // keeps the start of a mid-GOP join out of the demotion streak.
        if !self.anchored && !facts.is_idr {
            if !self.acked_anchor_message {
                self.acked_anchor_message = true;
                tracing::info!(target: "cedar",
                    "cedar: waiting for the first IDR before initializing the hardware decoder");
            }
            return Ok(None);
        }
        if !self.inited {
            let (width, height) = self.pending_dims.ok_or_else(|| {
                anyhow!("cedar: a decodable AU arrived before any SPS gave dimensions")
            })?;
            self.initialize(width, height)?;
        }
        // Feed first, then drain: one-in/one-out normally, and a frame the
        // SBM-full drain produced rides the same FIFO so display order never
        // inverts.
        let begin = self.profile.as_ref().map(|_| Instant::now());
        if let Some(frame) = self.feed_au(au, &facts)? {
            self.pending.push_back(frame);
        }
        if let Some(profile) = self.profile.as_mut() {
            profile.note_stage(1, elapsed_us(begin));
        }
        if facts.is_idr {
            self.anchored = true;
        }
        if self.append_aud { self.submit_aud_delimiter()?; }
        if let Some(frame) = self.drain()? {
            self.pending.push_back(frame);
        }
        if self.profile.is_some() && self.aus.is_multiple_of(120) {
            tracing::info!(target: "cedar", au = self.aus, fifo = self.output_fifo,
                pending = self.pending.len(), "cedar-output-queue");
        }
        let out = self.pending.pop_front();
        if out.is_some() {
            self.frames_out += 1;
        } else {
            self.empties += 1;
        }
        self.maybe_log();
        Ok(out.map(|output| {
            self.output_stamp = output.stamp;
            output.frame
        }))
    }

    /// IDR, colour, recovery for this AU from the shared planner — the same
    /// fold the software rung runs, so the two decoders cannot disagree on
    /// signalling.
    fn plan_facts(&mut self, au: &[u8]) -> Result<CedarFacts> {
        match self.planner.plan_au(au) {
            Ok(plan) => {
                if let Some(shape) = unsupported_shape(
                    plan.picture.chroma_format_idc,
                    plan.picture.bit_depth_luma_minus8,
                ) {
                    return Err(NoSoftwareRung {
                        codec: punktfunk_core::quic::CODEC_H264,
                        shape: Some(shape),
                    }
                    .into());
                }
                let safe = low_delay::safe_stream(plan.sps.pic_order_cnt_type,
                    plan.sps.frame_mbs_only_flag,
                    plan.slices.iter().any(|slice| slice.header.slice_type.is_b()));
                if self.pending_dims.is_none() {
                    tracing::info!(target: "cedar", poc_type = plan.sps.pic_order_cnt_type,
                        progressive = plan.sps.frame_mbs_only_flag, zero_reorder_verified = safe,
                        "cedar-stream-order");
                }
                if self.low_delay && !safe { bail!("cedar: low-delay candidate refuses a reordered or interlaced AU"); }
                self.zero_reorder_verified = safe;
                self.pending_dims = Some((plan.picture.coded_width, plan.picture.coded_height));
                let c = plan.picture.colour;
                let mark = self.recovery.note_h264(
                    plan.picture.frame_num,
                    plan.picture.is_idr,
                    plan.picture.recovery_point,
                );
                Ok(CedarFacts {
                    is_idr: plan.picture.is_idr,
                    color: Some(ColorDesc {
                        primaries: c.colour_primaries,
                        transfer: c.transfer_characteristics,
                        matrix: c.matrix_coefficients,
                        full_range: c.video_full_range,
                    }),
                    recovery: punktfunk_core::reanchor::LocalRecovery {
                        sei_here: mark.sei_here,
                        is_recovery_point: mark.is_recovery_point,
                    },
                })
            }
            Err(e) => {
                if self.low_delay { bail!("cedar: low-delay candidate cannot validate AU: {e}"); }
                if !matches!(e, PlanError::NoActiveParamSet { .. }) && !self.plan_warned {
                    self.plan_warned = true;
                    tracing::warn!(target: "cedar", error = %e,
                        "cedar: an AU did not plan — colour signalling follows the last AU that did");
                }
                Ok(CedarFacts {
                    is_idr: false,
                    color: None,
                    recovery: punktfunk_core::reanchor::LocalRecovery::NONE,
                })
            }
        }
    }

    /// `InitializeVideoDecoder` with the TSPS configuration: stream packages
    /// (raw Annex-B, matching the vendor demo), vendor-default frame-buffer
    /// count, smooth/display holding of 2, no non-demo knobs.
    fn initialize(&mut self, width: u32, height: u32) -> Result<()> {
        let tune = CedarTuning::from_lookup(|k| std::env::var(k).ok()).map_err(|e| anyhow!(e))?;
        self.drop_b_delay = tune.drop_b_delay;
        self.poll_budget_us = tune.poll_budget_us as u64;
        self.append_aud = tune.append_aud == 1;
        if tune.low_delay != 0 {
            let vendor_ok = std::fs::read("/usr/lib/libawh264.so").ok()
                .is_some_and(|vendor| low_delay::supports_vendor(&vendor));
            self.low_delay = low_delay::select(tune.low_delay, self.zero_reorder_verified, vendor_ok)
                .map_err(|e| anyhow!(e))?;
            tracing::info!(target: "cedar", mode = tune.low_delay, vendor_verified = vendor_ok,
                stream_verified = self.zero_reorder_verified, enabled = self.low_delay,
                "cedar-low-delay-selection");
        }
        // SAFETY: plain-data structs of integers and pointers; an all-zero
        // value is the vendor header's own "unset" state.
        let mut info: VideoStreamInfo = unsafe { std::mem::zeroed() };
        info.e_codec_format = VIDEO_CODEC_FORMAT_H264;
        info.n_width = width as c_int;
        info.n_height = height as c_int;
        info.n_frame_rate = 30;
        info.n_frame_duration = 33_333;
        // Default Annex-B SBM parsing is the validated baseline. The earlier
        // negative frame-package test preceded the ABI repair; retest only by
        // explicit one-axis candidate override on this now-correct layout.
        info.b_is_frame_package = tune.frame_package;

        // SAFETY: as above; the tail keeps the vendor's `sizeof(VConfig)`
        // memcpy inside this allocation.
        let mut storage: VConfigBuf = unsafe { std::mem::zeroed() };
        // Keep this set equal to what the device's vdecoderDemo drives:
        // planar-420 output (the demo prints eOutputPixelFormat = 1) and the
        // three holding counts. Everything else stays zeroed -- every extra
        // knob (frame-buffer count, SBM malloc mode, palloc-before-decode)
        // is unvalidated on this lib and the demo runs without them.
        if self.low_delay { storage.config.common_config_flags_192 = 1; }
        storage.config.e_output_pixel_format = PIXEL_FORMAT_YUV_PLANER_420;
        storage.config.n_de_interlace_holding_frame_buffer_num = 2;
        storage.config.b_no_b_frames = tune.no_b_frames;
        storage.config.n_display_holding_frame_buffer_num = tune.display;
        storage.config.n_decode_smooth_frame_buffer_num = tune.smooth;
        tracing::info!(target: "cedar", low_delay = self.low_delay, no_b_frames = tune.no_b_frames,
            frame_package = tune.frame_package, smooth = tune.smooth, display = tune.display,
            drop_b_delay = tune.drop_b_delay, immediate_handoff = self.immediate_handoff,
            poll_us = tune.poll_budget_us, append_aud = self.append_aud,
            "cedar: candidate configuration (one-axis A/B controls)");

        // SAFETY: `handle` is live; `info` and `storage` are live locals the
        // call only reads (and rewrites `storage.config.memops` through, as
        // the vendor adapter does). The lib may copy `sizeof(VConfig)` bytes
        // from `&mut storage.config` — the tail covers that read.
        let rc = unsafe { (self.libs.init)(self.handle, &mut info, &mut storage.config) };
        ensure!(
            rc == 0,
            "cedar: InitializeVideoDecoder rc={rc} ({width}x{height})"
        );
        self.inited = true;
        tracing::info!(target: "cedar", width, height,
            "cedar: hardware decoder initialized (H.264, YUV_PLANER_420 out, vendor-default buffers)");
        Ok(())
    }

    /// Copy one AU into the vendor stream buffer and submit it as one
    /// complete frame package. The returned frame, if any, is one the SBM-full
    /// retry had to retire before this AU could be submitted — the caller
    /// queues it ahead of this AU's own drain.
    fn feed_au(&mut self, au: &[u8], facts: &CedarFacts) -> Result<Option<CedarOutput>> {
        ensure!(
            au.len() <= MAX_AU_BYTES,
            "cedar: {} byte AU exceeds the SBM's addressable length",
            au.len()
        );
        let len = au.len() as c_int;
        let mut buf: *mut c_char = std::ptr::null_mut();
        let mut buf_size: c_int = 0;
        let mut ring: *mut c_char = std::ptr::null_mut();
        let mut ring_size: c_int = 0;
        // SAFETY: `handle` is live and the four out-pointers are live locals
        // for the duration of the call, which is what the header documents.
        let mut rc = unsafe {
            (self.libs.request_stream_buffer)(
                self.handle,
                len,
                &mut buf,
                &mut buf_size,
                &mut ring,
                &mut ring_size,
                STREAM_INDEX,
            )
        };
        let mut retired: Option<CedarOutput> = None;
        if rc < 0 {
            // The SBM is full: retire submitted data with one drain, hold any
            // frame it produced for the caller, and retry once.
            retired = self.drain()?;
            rc = unsafe {
                (self.libs.request_stream_buffer)(
                    self.handle,
                    len,
                    &mut buf,
                    &mut buf_size,
                    &mut ring,
                    &mut ring_size,
                    STREAM_INDEX,
                )
            };
        }
        ensure!(
            rc >= 0 && !buf.is_null() && buf_size > 0,
            "cedar: RequestVideoStreamBuffer refused {len} bytes (SBM full)"
        );
        let first = (buf_size as usize).min(au.len());
        ensure!(
            au.len() <= buf_size as usize + ring_size as usize,
            "cedar: AU of {} bytes exceeds the SBM free segment ({} + {})",
            au.len(),
            buf_size,
            ring_size
        );
        // SAFETY: `buf` spans `buf_size` writable bytes and `ring` spans
        // `ring_size`; the split copies exactly `au.len()` bytes into those
        // two segments that the request call just returned.
        unsafe {
            std::ptr::copy_nonoverlapping(au.as_ptr(), buf.cast::<u8>(), first);
            if first < au.len() {
                std::ptr::copy_nonoverlapping(
                    au.as_ptr().add(first),
                    ring.cast::<u8>(),
                    au.len() - first,
                );
            }
        }
        // SAFETY: plain-data struct; the vendor reads it during the submit
        // call below and does not retain the local.
        let mut data = unsafe { std::mem::zeroed::<VideoStreamDataInfo>() };
        data.p_data = buf;
        data.n_length = len;
        let stamp = self.input_stamp.take();
        let capture_ns = stamp.map_or(self.now_us().max(1) as u64 * 1000, |s| s.pts_ns);
        data.n_pts = if self.pts_probe { self.aus as i64 * 1000 } else {
            self.pts_clock.next(capture_ns).ok_or_else(|| anyhow!("cedar: PTS token exhausted"))?
        };
        self.pts_ledger.insert(data.n_pts, SubmittedPicture {
            source_au: self.aus, stamp, facts: *facts, color: self.color,
        }).map_err(|e| anyhow!(e))?;
        data.b_is_first_part = 1;
        data.b_is_last_part = 1;
        data.b_valid = 1;
        // SAFETY: `handle` is live and `data` is a live local for the call.
        let rc = unsafe { (self.libs.submit_stream_data)(self.handle, &mut data, STREAM_INDEX) };
        if rc < 0 {
            self.pts_ledger.take(data.n_pts);
            bail!("cedar: SubmitVideoStreamData rc={rc}");
        }
        if (self.pts_probe || self.profile.is_some()) && (self.aus <= 16 || self.aus.is_multiple_of(120)) {
            tracing::info!(target: "cedar", au = self.aus, pts = data.n_pts,
                capture_ns, outstanding = self.pts_ledger.len(), "cedar-pts-submit");
        }
        Ok(retired)
    }

    /// Explicit non-VCL boundary packet. It has no picture identity and never
    /// enters the PTS ledger. The original coded AU and its token stay unchanged.
    fn submit_aud_delimiter(&mut self) -> Result<()> {
        let bytes = async_parser::AUD_DELIMITER;
        let mut buf: *mut c_char = std::ptr::null_mut();
        let mut ring: *mut c_char = std::ptr::null_mut();
        let mut first_len = 0;
        let mut ring_len = 0;
        // SAFETY: live decoder and writable scalar out-pointers; same ABI as feed_au.
        let rc = unsafe { (self.libs.request_stream_buffer)(self.handle, bytes.len() as c_int,
            &mut buf, &mut first_len, &mut ring, &mut ring_len, STREAM_INDEX) };
        ensure!(rc >= 0 && first_len >= 0 && ring_len >= 0,
            "cedar: delimiter buffer request failed");
        ensure!(bytes.len() <= first_len as usize + ring_len as usize,
            "cedar: delimiter buffer span too small");
        let first = bytes.len().min(first_len as usize);
        ensure!(first == 0 || !buf.is_null(), "cedar: delimiter primary buffer NULL");
        ensure!(first == bytes.len() || !ring.is_null(), "cedar: delimiter ring buffer NULL");
        // SAFETY: above checks bound both requested segments and validate their pointers.
        unsafe {
            if first > 0 { std::ptr::copy_nonoverlapping(bytes.as_ptr(), buf.cast::<u8>(), first); }
            if first < bytes.len() { std::ptr::copy_nonoverlapping(bytes.as_ptr().add(first),
                ring.cast::<u8>(), bytes.len() - first); }
        }
        // SAFETY: plain C data, valid zero/default pointers, filled before the submit call.
        let mut data = unsafe { std::mem::zeroed::<VideoStreamDataInfo>() };
        data.p_data = buf;
        data.n_length = bytes.len() as c_int;
        data.n_pts = async_parser::AUD_PTS;
        data.n_pcr = -1;
        data.b_is_first_part = 1;
        data.b_is_last_part = 1;
        data.b_valid = 1;
        data.n_stream_index = STREAM_INDEX;
        // SAFETY: same live handle/data contract as an ordinary AU submission.
        let rc = unsafe { (self.libs.submit_stream_data)(self.handle, &mut data, STREAM_INDEX) };
        ensure!(rc == 0, "cedar: delimiter submit failed rc={rc}");
        Ok(())
    }

    /// Run the vendor decoder until it has nothing more to do. Preserve every
    /// picture in the bounded output FIFO; newest-wins is diagnostic control only.
    fn drain(&mut self) -> Result<Option<CedarOutput>> {
        let mut newest: Option<CedarOutput> = None;
        let mut produced = 0usize;
        let async_start = (self.poll_budget_us > 0).then(Instant::now);
        for _ in 0..DRAIN_ROUNDS {
            let begin = self.profile.as_ref().map(|_| Instant::now());
            // SAFETY: `handle` is live; the flags are the live-stream contract
            // (not end-of-stream, any picture, B-frames not expected on the
            // wire) and the clock is this decoder's monotonic microsecond time.
            let rc = unsafe { (self.libs.decode)(self.handle, 0, 0, self.drop_b_delay, self.now_us()) };
            if let Some(profile) = self.profile.as_mut() {
                profile.note_vendor(rc, elapsed_us(begin));
            }
            match rc {
                VDECODE_RESULT_FRAME_DECODED | VDECODE_RESULT_KEYFRAME_DECODED => {
                    if let Some(frame) = self.take_picture()? {
                        produced += 1;
                        ensure!(!self.output_fifo || self.pending.len() < 32,
                            "cedar: output FIFO exceeded 32 pictures; refuse unbounded backlog");
                        if queue_picture(&mut self.pending, &mut newest, frame, self.output_fifo) {
                            if let Some(profile) = self.profile.as_mut() {
                                profile.replaced_pictures += 1;
                            }
                        }
                    }
                }
                VDECODE_RESULT_OK => {}
                VDECODE_RESULT_CONTINUE | VDECODE_RESULT_NO_BITSTREAM => {
                    // The SBM parser runs on its own thread. An immediate empty
                    // result does not mean the submitted complete AU is unavailable
                    // until the next network frame; allow an opt-in bounded retry.
                    if async_parser::retry_async(rc, produced, elapsed_us(async_start), self.poll_budget_us) {
                        std::thread::sleep(std::time::Duration::from_micros(200));
                        continue;
                    }
                    break;
                },
                VDECODE_RESULT_NO_FRAME_BUFFER => {
                    // Output buffers are all held — every picture this rung
                    // takes is returned right after its copy, so this is the
                    // vendor's own pipeline depth, not ours to release.
                    tracing::debug!(target: "cedar",
                        "cedar: decoder reported NO_FRAME_BUFFER (pipeline depth)");
                    break;
                }
                VDECODE_RESULT_RESOLUTION_CHANGE => {
                    bail!("cedar: mid-stream resolution change (reopen unsupported in this build)");
                }
                other => bail!("cedar: DecodeVideoStream rc={other}"),
            }
        }
        if let Some(profile) = self.profile.as_mut() {
            profile.note_drain(produced);
        }
        Ok(newest)
    }

    fn take_picture(&mut self) -> Result<Option<CedarOutput>> {
        let begin = self.profile.as_ref().map(|_| Instant::now());
        // SAFETY: `handle` is live; NULL means no display picture is pending.
        let pic = unsafe { (self.libs.request_picture)(self.handle, STREAM_INDEX) };
        if let Some(profile) = self.profile.as_mut() {
            profile.note_stage(3, elapsed_us(begin));
            if pic.is_null() { profile.empty_pictures += 1; }
        }
        if pic.is_null() {
            return Ok(None);
        }
        let begin = self.profile.as_ref().map(|_| Instant::now());
        let copied = self.copy_picture(pic);
        if let Some(profile) = self.profile.as_mut() {
            profile.note_copy(elapsed_us(begin));
        }
        let begin = self.profile.as_ref().map(|_| Instant::now());
        // SAFETY: `pic` is the picture RequestPicture handed over; the return
        // call releases it back to the frame buffer manager exactly once, on
        // every path — including a failed copy above.
        let rc = unsafe { (self.libs.return_picture)(self.handle, pic) };
        if let Some(profile) = self.profile.as_mut() {
            profile.note_stage(5, elapsed_us(begin));
        }
        if rc != 0 {
            tracing::warn!(target: "cedar", rc,
                "cedar: ReturnPicture refused a picture from this FBM");
        }
        copied.map(Some)
    }

    fn copy_picture(&mut self, pic: *const VideoPicture) -> Result<CedarOutput> {
        // SAFETY: `pic` is the live picture the vendor just handed over; every
        // read below is a scalar or a plane range the header documents, and
        // the pointer stays valid until ReturnPicture (already en route).
        let p = unsafe { &*pic };
        let source = self.pts_ledger.take(p.n_pts);
        if source.is_some() { self.pts_matches += 1; } else { self.pts_unmatched += 1; }
        if ((self.pts_probe || self.profile.is_some()) && (self.aus <= 16 || self.aus.is_multiple_of(120)))
            || source.is_none() {
            tracing::info!(target: "cedar", input_au = self.aus,
                source_au = ?source.map(|s| s.source_au), pts = p.n_pts,
                matches = self.pts_matches, unmatched = self.pts_unmatched,
                outstanding = self.pts_ledger.len(), "cedar-pts-output");
        }
        let source = source.ok_or_else(|| anyhow!(
            "cedar: unmatched output PTS; refusing FIFO-order guess (pts={})", p.n_pts))?;
        phases::note_lag(&mut self.output_lag_frames, self.aus, source.source_au);
        if self.profile.is_some() && (self.aus <= 8 || self.aus.is_multiple_of(120)) {
            tracing::info!(target: "cedar", au = self.aus, slot = p.n_id, pts = p.n_pts,
                progressive = p.b_is_progressive, top_field_first = p.b_top_field_first,
                frame_error = p.b_frame_error_flag, fd = p.n_buf_fd,
                "cedar-picture-metadata (slot is not AU identity)");
        }
        let width = p.n_width.max(0) as u32;
        let height = p.n_height.max(0) as u32;
        ensure!(width > 0 && height > 0, "cedar: empty picture {width}x{height}");
        let stride = p.n_line_stride.max(width as c_int) as usize;
        let left = p.n_left_offset.max(0) as usize;
        let top = p.n_top_offset.max(0) as usize;
        if !self.offsets_logged
            && (left != 0 || top != 0 || p.n_right_offset != 0 || p.n_bottom_offset != 0)
        {
            self.offsets_logged = true;
            tracing::info!(target: "cedar",
                left, top, right = p.n_right_offset, bottom = p.n_bottom_offset,
                "cedar: picture crop offsets in play");
        }
        let (cw, ch) = CpuPlanarFrame::chroma_dims(width, height);
        let chroma_stride = stride / 2;
        let y_len = (width as usize)
            .checked_mul(height as usize)
            .ok_or_else(|| anyhow!("cedar: luma size overflow"))?;
        let c_len = (cw as usize)
            .checked_mul(ch as usize)
            .ok_or_else(|| anyhow!("cedar: chroma size overflow"))?;
        let total_len = y_len
            .checked_add(
                c_len
                    .checked_mul(2)
                    .ok_or_else(|| anyhow!("cedar: I420 size overflow"))?,
            )
            .ok_or_else(|| anyhow!("cedar: I420 size overflow"))?;

        // Allocate the final I420 payload once. Keep len=0 until every byte has
        // been written, so an error cannot expose uninitialized storage.
        let mut packed = Vec::<u8>::with_capacity(total_len);
        let y_dst = packed.as_mut_ptr();
        // SAFETY: the allocation has total_len capacity; these pointers stay
        // within its three final I420 plane ranges and are only written below.
        let (u_dst, v_dst) = unsafe { (y_dst.add(y_len), y_dst.add(y_len + c_len)) };

        match p.e_pixel_format {
            PIXEL_FORMAT_YUV_PLANER_420 => {
                copy_plane_into(y_dst, p.p_data0, stride, left, top, width, height)?;
                copy_plane_into(
                    u_dst,
                    p.p_data1,
                    chroma_stride,
                    left / 2,
                    top / 2,
                    cw,
                    ch,
                )?;
                copy_plane_into(
                    v_dst,
                    p.p_data2,
                    chroma_stride,
                    left / 2,
                    top / 2,
                    cw,
                    ch,
                )?;
            }
            PIXEL_FORMAT_YV12 => {
                copy_plane_into(y_dst, p.p_data0, stride, left, top, width, height)?;
                copy_plane_into(
                    u_dst,
                    p.p_data2,
                    chroma_stride,
                    left / 2,
                    top / 2,
                    cw,
                    ch,
                )?;
                copy_plane_into(
                    v_dst,
                    p.p_data1,
                    chroma_stride,
                    left / 2,
                    top / 2,
                    cw,
                    ch,
                )?;
            }
            PIXEL_FORMAT_NV12 | PIXEL_FORMAT_NV21 => {
                copy_plane_into(y_dst, p.p_data0, stride, left, top, width, height)?;
                copy_interleaved_chroma_into(
                    u_dst,
                    v_dst,
                    p.p_data1,
                    stride,
                    left / 2,
                    top / 2,
                    cw,
                    ch,
                    p.e_pixel_format == PIXEL_FORMAT_NV12,
                )?;
            }
            other => bail!("cedar: output pixel format {other} is not a copyable 4:2:0 layout"),
        }

        // SAFETY: each supported branch above writes exactly y_len + 2*c_len
        // bytes into the allocation before this point.
        unsafe { packed.set_len(total_len) };
        if !self.format_logged {
            self.format_logged = true;
            tracing::info!(target: "cedar",
                format = p.e_pixel_format, width, height, stride,
                "cedar: first picture decoded (vendor format copied directly to packed I420)");
        }
        let mut frame = CpuPlanarFrame::from_packed_i420(
            width,
            height,
            packed,
            source.color,
            source.facts.is_idr,
            DECODER_PIN,
        )?;
        frame.recovery = source.facts.recovery;
        let stamp = source.stamp.map(|mut s| {
            s.ready_ns = punktfunk_core::quic::wall_clock_ns();
            s
        });
        Ok(CedarOutput { frame, stamp })
    }

    fn maybe_log(&mut self) {
        if self.frames_out == 0 {
            return;
        }
        if self.frames_out == 1 {
            tracing::info!(target: "cedar",
                aus = self.aus, "cedar: first hardware frame delivered to the presenter path");
        }
        if self.frames_out - self.logged_at >= LOG_EVERY_FRAMES || self.frames_out == 1 {
            self.logged_at = self.frames_out;
            tracing::info!(target: "cedar",
                aus = self.aus,
                frames = self.frames_out,
                empties = self.empties,
                errors = self.errors,
                "cedar: decode cadence");
        }
    }

    fn now_us(&self) -> i64 {
        self.start.elapsed().as_micros() as i64
    }
}

impl Drop for NativeCedarDecoder {
    fn drop(&mut self) {
        if !self.handle.is_null() {
            // SAFETY: `handle` came from CreateVideoDecoder and this is its
            // sole owner (no Clone, no Sync); destroy runs exactly once.
            unsafe { (self.libs.destroy)(self.handle) };
            self.handle = std::ptr::null_mut();
        }
        tracing::info!(target: "cedar",
            aus = self.aus, frames = self.frames_out, empties = self.empties, errors = self.errors,
            pts_matches = self.pts_matches, pts_unmatched = self.pts_unmatched,
            pts_outstanding = self.pts_ledger.len(),
            output_lag_frames = ?self.output_lag_frames,
            "cedar: decoder closed");
    }
}

/// 8-bit 4:2:0 only — the same envelope the CPU rung enforces, from the same
/// planner, reported with the same typed refusal.
fn unsupported_shape(chroma_format_idc: u8, bit_depth_minus8: u8) -> Option<&'static str> {
    if bit_depth_minus8 != 0 {
        return Some("10-bit or deeper");
    }
    if chroma_format_idc != punktfunk_core::quic::CHROMA_IDC_420 {
        return Some("chroma other than 4:2:0");
    }
    None
}

/// Copy one visible plane row-by-row from the vendor buffer (stride-padded)
/// into a tightly packed `Vec`. `None` base or a span shorter than the visible
/// region refuses instead of reading whatever is there.
fn copy_plane_into(
    dst: *mut u8,
    base: *const c_char,
    stride: usize,
    left: usize,
    top: usize,
    width: u32,
    height: u32,
) -> Result<()> {
    ensure!(!base.is_null(), "cedar: picture plane pointer is NULL");
    ensure!(!dst.is_null(), "cedar: picture destination pointer is NULL");
    let (w, h) = (width as usize, height as usize);
    ensure!(stride >= left + w, "cedar: plane stride {stride} < {left}+{w}");

    if left == 0 && top == 0 && stride == w {
        // The common TSPS 720p path is tightly packed: one memcpy instead of
        // 720 row copies and no temporary Vec.
        // SAFETY: the vendor owns at least w*h readable bytes for this plane,
        // and dst owns exactly that many writable bytes in the final I420 Vec.
        unsafe {
            std::ptr::copy_nonoverlapping(base.cast::<u8>(), dst, w * h);
        }
        return Ok(());
    }

    for row in 0..h {
        // SAFETY: the vendor contracts stride bytes per row; the geometry check
        // above keeps left..left+w inside each row, while dst spans w*h bytes.
        unsafe {
            std::ptr::copy_nonoverlapping(
                base.add((top + row) * stride + left).cast::<u8>(),
                dst.add(row * w),
                w,
            );
        }
    }
    Ok(())
}

/// Split one interleaved 4:2:0 chroma plane (NV12 = U first, NV21 = V first)
/// directly into the final packed I420 chroma ranges.
fn copy_interleaved_chroma_into(
    u_dst: *mut u8,
    v_dst: *mut u8,
    base: *const c_char,
    stride: usize,
    left: usize,
    top: usize,
    width: u32,
    height: u32,
    uv_first_is_u: bool,
) -> Result<()> {
    ensure!(!base.is_null(), "cedar: interleaved chroma pointer is NULL");
    ensure!(!u_dst.is_null() && !v_dst.is_null(), "cedar: chroma destination pointer is NULL");
    let (w, h) = (width as usize, height as usize);
    let span = left + 2 * w;
    ensure!(stride >= span, "cedar: chroma stride {stride} < {span}");

    for row in 0..h {
        // SAFETY: the vendor contracts stride bytes per row and span was
        // checked above; both output planes own w*h bytes.
        let src = unsafe { base.add((top + row) * stride + left).cast::<u8>() };
        let out = row * w;
        if uv_first_is_u {
            for col in 0..w {
                unsafe {
                    *u_dst.add(out + col) = *src.add(2 * col);
                    *v_dst.add(out + col) = *src.add(2 * col + 1);
                }
            }
        } else {
            for col in 0..w {
                unsafe {
                    *v_dst.add(out + col) = *src.add(2 * col);
                    *u_dst.add(out + col) = *src.add(2 * col + 1);
                }
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The six constants the session's `stats:` line and pin resolution key on.
    #[test]
    fn pin_name_is_the_public_pin() {
        assert_eq!(DECODER_PIN, "native-cedar");
    }

    /// The ABI transcriptions against the real device libvdecoder: the
    /// VideoConfig anchors are extracted from the shipped vdecoderDemo
    /// binary (holding counts at 0x68..0x74, memops at 0x80, nVeFreq at
    /// 0xA4, sizeof 216); the rest from the public headers on aarch64 LP64.
    /// A layout drift here is a wrong picture on the device, so it must
    /// fail at test time, not at the first stream.
    #[test]
    fn abi_layouts_match_the_device_abi() {
        use std::mem::offset_of;
        use std::mem::size_of;

        assert_eq!(size_of::<VideoStreamInfo>(), 64);
        assert_eq!(offset_of!(VideoStreamInfo, p_codec_specific_data), 32);
        assert_eq!(offset_of!(VideoStreamInfo, b_is_frame_package), 48);

        assert_eq!(size_of::<VideoConfig>(), 216);
        assert_eq!(offset_of!(VideoConfig, common_config_flags_192), 192);
        assert_eq!(offset_of!(VideoConfig, e_output_pixel_format), 36);
        assert_eq!(offset_of!(VideoConfig, b_no_b_frames), 44);
        assert_eq!(offset_of!(VideoConfig, n_frame_buffer_num), 64);
        assert_eq!(offset_of!(VideoConfig, n_align_stride), 88);
        assert_eq!(offset_of!(VideoConfig, b_is_soft_decoder_flag), 92);
        assert_eq!(offset_of!(VideoConfig, b_vir_malloc_sbm), 96);
        assert_eq!(
            offset_of!(VideoConfig, b_support_palloc_buf_before_decode),
            100
        );
        assert_eq!(
            offset_of!(VideoConfig, n_de_interlace_holding_frame_buffer_num),
            104
        );
        assert_eq!(
            offset_of!(VideoConfig, n_display_holding_frame_buffer_num),
            108
        );
        assert_eq!(
            offset_of!(VideoConfig, n_rotate_holding_frame_buffer_num),
            112
        );
        assert_eq!(
            offset_of!(VideoConfig, n_decode_smooth_frame_buffer_num),
            116
        );
        assert_eq!(offset_of!(VideoConfig, memops), 128);
        assert_eq!(offset_of!(VideoConfig, n_ve_freq), 164);

        assert_eq!(size_of::<VideoStreamDataInfo>(), 64);
        assert_eq!(offset_of!(VideoStreamDataInfo, n_pts), 16);
        assert_eq!(offset_of!(VideoStreamDataInfo, p_video_info), 56);

        assert_eq!(size_of::<VideoFrmStatusInfo>(), 72);
        assert_eq!(size_of::<VideoPicture>(), 320);
        assert_eq!(offset_of!(VideoPicture, e_pixel_format), 8);
        assert_eq!(offset_of!(VideoPicture, n_line_stride), 20);
        assert_eq!(offset_of!(VideoPicture, p_data0), 80);
        assert_eq!(offset_of!(VideoPicture, p_data2), 96);
        assert_eq!(offset_of!(VideoPicture, n_buf_fd), 168);
        assert_eq!(offset_of!(VideoPicture, colour_primaries), 212);
    }

    /// The 8-bit 4:2:0 envelope, same answers as the software rung's copy.
    #[test]
    fn shape_gate_matches_the_cpu_rung() {
        use punktfunk_core::quic::{CHROMA_IDC_420, CHROMA_IDC_444};
        assert_eq!(unsupported_shape(CHROMA_IDC_420, 0), None);
        assert_eq!(unsupported_shape(CHROMA_IDC_420, 2), Some("10-bit or deeper"));
        assert_eq!(
            unsupported_shape(CHROMA_IDC_444, 0),
            Some("chroma other than 4:2:0")
        );
        assert_eq!(
            unsupported_shape(CHROMA_IDC_444, 2),
            Some("10-bit or deeper")
        );
    }

    /// The vectorised copies must produce packed planes from strided rows,
    /// with the `NV12`/`NV21` split honouring the first-byte order.
    #[test]
    fn plane_copies_pack_and_split() {
        // 4x2 luma, stride 6: rows "abcdef", "ghijkl" — columns 1..5 visible.
        let y_src: [u8; 12] = *b"abcdefghijkl";
        let y = copy_plane(y_src.as_ptr().cast(), 6, 1, 0, 4, 2).expect("luma");
        assert_eq!(y, b"bcdeghij".to_vec());

        // 2x2 chroma rows, stride 6, interleaved NV12 (U first).
        let uv: [u8; 12] = [1, 2, 3, 4, 0, 0, 5, 6, 7, 8, 0, 0];
        let (u, v) = copy_interleaved_chroma(uv.as_ptr().cast(), 6, 0, 0, 2, 2, true).expect("nv12");
        assert_eq!(u, vec![1, 3, 5, 7]);
        assert_eq!(v, vec![2, 4, 6, 8]);
        // NV21: V first.
        let (u, v) = copy_interleaved_chroma(uv.as_ptr().cast(), 6, 0, 0, 2, 2, false).expect("nv21");
        assert_eq!(v, vec![1, 3, 5, 7]);
        assert_eq!(u, vec![2, 4, 6, 8]);
    }
}
