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
use phases::{PhaseStats, elapsed_us};

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
    /// Opaque tail: the device's VConfig is 216 bytes; the fields past
    /// `n_channel_num` are not transcribed (never touched here). This pad
    /// makes `size_of` match the device so the tests can pin it.
    _tail: [u8; 32],
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
    pending: VecDeque<CpuPlanarFrame>,
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
            start: Instant::now(),
        })
    }

    /// The native-rung recovery channel. Cedar concealment surfaces as `Err`s
    /// (which already request a keyframe through the demotion streak), so this
    /// rung has nothing extra to report.
    pub(crate) fn take_recovery_request(&mut self) -> bool {
        false
    }

    pub(crate) fn decode(&mut self, au: &[u8]) -> Result<Option<CpuPlanarFrame>> {
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
        if let Some(frame) = self.drain(&facts)? {
            self.pending.push_back(frame);
        }
        let out = self.pending.pop_front();
        if out.is_some() {
            self.frames_out += 1;
        } else {
            self.empties += 1;
        }
        self.maybe_log();
        Ok(out)
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
        // SAFETY: plain-data structs of integers and pointers; an all-zero
        // value is the vendor header's own "unset" state.
        let mut info: VideoStreamInfo = unsafe { std::mem::zeroed() };
        info.e_codec_format = VIDEO_CODEC_FORMAT_H264;
        info.n_width = width as c_int;
        info.n_height = height as c_int;
        info.n_frame_rate = 30;
        info.n_frame_duration = 33_333;
        // Stream packages, not frame packages: the A523 libvdecoder did not
        // decode anything from a frame-package feed (631 AUs, 0 frames, SBM
        // full), and every vendor consumer of raw Annex-B on this platform --
        // vdecoderDemo included -- leaves this flag unset (0).
        info.b_is_frame_package = 0;

        // SAFETY: as above; the tail keeps the vendor's `sizeof(VConfig)`
        // memcpy inside this allocation.
        let mut storage: VConfigBuf = unsafe { std::mem::zeroed() };
        // Keep this set equal to what the device's vdecoderDemo drives:
        // planar-420 output (the demo prints eOutputPixelFormat = 1) and the
        // three holding counts. Everything else stays zeroed -- every extra
        // knob (frame-buffer count, SBM malloc mode, palloc-before-decode)
        // is unvalidated on this lib and the demo runs without them.
        storage.config.e_output_pixel_format = PIXEL_FORMAT_YUV_PLANER_420;
        storage.config.n_de_interlace_holding_frame_buffer_num = 2;
        storage.config.n_display_holding_frame_buffer_num = 2;
        storage.config.n_decode_smooth_frame_buffer_num = 2;

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
    fn feed_au(&mut self, au: &[u8], facts: &CedarFacts) -> Result<Option<CpuPlanarFrame>> {
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
        let mut retired: Option<CpuPlanarFrame> = None;
        if rc < 0 {
            // The SBM is full: retire submitted data with one drain, hold any
            // frame it produced for the caller, and retry once.
            retired = self.drain(facts)?;
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
        data.n_pts = -1;
        data.b_is_first_part = 1;
        data.b_is_last_part = 1;
        data.b_valid = 1;
        // SAFETY: `handle` is live and `data` is a live local for the call.
        let rc = unsafe { (self.libs.submit_stream_data)(self.handle, &mut data, STREAM_INDEX) };
        ensure!(rc >= 0, "cedar: SubmitVideoStreamData rc={rc}");
        Ok(retired)
    }

    /// Run the vendor decoder until it has nothing more to do, newest frame
    /// wins (the pump's queue rule).
    fn drain(&mut self, facts: &CedarFacts) -> Result<Option<CpuPlanarFrame>> {
        let mut newest: Option<CpuPlanarFrame> = None;
        let mut produced = 0usize;
        for _ in 0..DRAIN_ROUNDS {
            let begin = self.profile.as_ref().map(|_| Instant::now());
            // SAFETY: `handle` is live; the flags are the live-stream contract
            // (not end-of-stream, any picture, B-frames not expected on the
            // wire) and the clock is this decoder's monotonic microsecond time.
            let rc = unsafe { (self.libs.decode)(self.handle, 0, 0, 0, self.now_us()) };
            if let Some(profile) = self.profile.as_mut() {
                profile.note_vendor(rc, elapsed_us(begin));
            }
            match rc {
                VDECODE_RESULT_FRAME_DECODED | VDECODE_RESULT_KEYFRAME_DECODED => {
                    if let Some(frame) = self.take_picture(facts)? {
                        produced += 1;
                        if newest.is_some() {
                            if let Some(profile) = self.profile.as_mut() {
                                profile.replaced_pictures += 1;
                            }
                        }
                        newest = Some(frame);
                    }
                }
                VDECODE_RESULT_OK => {}
                VDECODE_RESULT_CONTINUE | VDECODE_RESULT_NO_BITSTREAM => break,
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

    fn take_picture(&mut self, facts: &CedarFacts) -> Result<Option<CpuPlanarFrame>> {
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
        let copied = self.copy_picture(pic, facts);
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

    fn copy_picture(&mut self, pic: *const VideoPicture, facts: &CedarFacts) -> Result<CpuPlanarFrame> {
        // SAFETY: `pic` is the live picture the vendor just handed over; every
        // read below is a scalar or a plane range the header documents, and
        // the pointer stays valid until ReturnPicture (already en route).
        let p = unsafe { &*pic };
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
        let (y, u, v) = match p.e_pixel_format {
            PIXEL_FORMAT_YUV_PLANER_420 => (
                copy_plane(p.p_data0, stride, left, top, width, height)?,
                copy_plane(p.p_data1, chroma_stride, left / 2, top / 2, cw, ch)?,
                copy_plane(p.p_data2, chroma_stride, left / 2, top / 2, cw, ch)?,
            ),
            PIXEL_FORMAT_YV12 => (
                copy_plane(p.p_data0, stride, left, top, width, height)?,
                copy_plane(p.p_data2, chroma_stride, left / 2, top / 2, cw, ch)?,
                copy_plane(p.p_data1, chroma_stride, left / 2, top / 2, cw, ch)?,
            ),
            PIXEL_FORMAT_NV12 | PIXEL_FORMAT_NV21 => {
                let y = copy_plane(p.p_data0, stride, left, top, width, height)?;
                let uv_first_is_u = p.e_pixel_format == PIXEL_FORMAT_NV12;
                let (u, v) = copy_interleaved_chroma(
                    p.p_data1,
                    stride,
                    left / 2,
                    top / 2,
                    cw,
                    ch,
                    uv_first_is_u,
                )?;
                (y, u, v)
            }
            other => bail!("cedar: output pixel format {other} is not a copyable 4:2:0 layout"),
        };
        if !self.format_logged {
            self.format_logged = true;
            tracing::info!(target: "cedar",
                format = p.e_pixel_format, width, height, stride,
                "cedar: first picture decoded (vendor format copied to packed I420)");
        }
        let mut frame =
            CpuPlanarFrame::from_planes(width, height, [y, u, v], self.color, facts.is_idr, DECODER_PIN)?;
        frame.recovery = facts.recovery;
        Ok(frame)
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
fn copy_plane(
    base: *const c_char,
    stride: usize,
    left: usize,
    top: usize,
    width: u32,
    height: u32,
) -> Result<Vec<u8>> {
    ensure!(!base.is_null(), "cedar: picture plane pointer is NULL");
    let (w, h) = (width as usize, height as usize);
    ensure!(stride >= left + w, "cedar: plane stride {stride} < {left}+{w}");
    let mut out = Vec::with_capacity(w * h);
    for row in 0..h {
        // SAFETY: the vendor contracts that each plane spans `stride` bytes
        // per row from `base` for at least the visible height; row `top + row`
        // and columns `left..left + w` are inside that span by the checks
        // above.
        let src = unsafe { std::slice::from_raw_parts(base.add((top + row) * stride + left).cast::<u8>(), w) };
        out.extend_from_slice(src);
    }
    Ok(out)
}

/// Split one interleaved 4:2:0 chroma plane (`NV12` = U first, `NV21` = V
/// first) into two tightly packed planes.
fn copy_interleaved_chroma(
    base: *const c_char,
    stride: usize,
    left: usize,
    top: usize,
    width: u32,
    height: u32,
    uv_first_is_u: bool,
) -> Result<(Vec<u8>, Vec<u8>)> {
    ensure!(!base.is_null(), "cedar: interleaved chroma pointer is NULL");
    let (w, h) = (width as usize, height as usize);
    let span = left + 2 * w;
    ensure!(stride >= span, "cedar: chroma stride {stride} < {span}");
    let mut u = Vec::with_capacity(w * h);
    let mut v = Vec::with_capacity(w * h);
    for row in 0..h {
        // SAFETY: as `copy_plane`, with the interleaved pair width: the vendor
        // contracts `stride` bytes per row and the row range spans
        // `left..left + 2*w` inside it.
        let src = unsafe { std::slice::from_raw_parts(base.add((top + row) * stride + left).cast::<u8>(), 2 * w) };
        for pair in src.chunks_exact(2) {
            let (first, second) = (pair[0], pair[1]);
            if uv_first_is_u {
                u.push(first);
                v.push(second);
            } else {
                v.push(first);
                u.push(second);
            }
        }
    }
    Ok((u, v))
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
        assert_eq!(offset_of!(VideoConfig, e_output_pixel_format), 36);
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
