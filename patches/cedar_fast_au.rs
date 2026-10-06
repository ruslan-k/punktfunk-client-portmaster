//! Cheap access-unit classification for the Cedar rung.
//!
//! The full H.264 planner resolves colour, recovery, shape and slice facts and
//! runs on every access unit. An ordinary P AU carries none of that state: its
//! only facts are the slice types (the low-delay gate refuses B slices) and the
//! frame number (the recovery tracker counts increments). This scanner reads
//! just those two and answers `NeedsFullPlan` for everything else - SPS, PPS,
//! SEI, IDR, a partition NAL, an AU with no VCL NAL, or any header it cannot
//! finish - so the authoritative planner keeps deciding anything that can
//! change state.
//!
//! The scanner is deliberately conservative: refusing costs one full plan, while
//! accepting wrongly would mis-attribute recovery facts or arm the hardware gate
//! on a reordered stream.

/// SPS fields the cheap path needs to size the slice header.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct SpsBits {
    pub log2_max_frame_num_minus4: u8,
    pub separate_colour_plane: bool,
}

/// What the scanner decided about one AU.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum AuClass {
    /// Every VCL NAL is a non-IDR slice, and no other NAL needs the planner.
    Plain { has_b_slice: bool, frame_num: u16 },
    /// SPS/PPS/SEI/IDR, no VCL NAL, or a header the scanner refused.
    NeedsFullPlan,
}

const NAL_SLICE: u8 = 1;
const NAL_IDR: u8 = 5;
const NAL_SEI: u8 = 6;
const NAL_SPS: u8 = 7;
const NAL_PPS: u8 = 8;
const NAL_AUD: u8 = 9;
const NAL_END_SEQ: u8 = 10;
const NAL_END_STREAM: u8 = 11;
const NAL_FILLER: u8 = 12;

/// One NAL unit's type and payload, without its start code.
fn nal_units(au: &[u8]) -> Vec<(u8, &[u8])> {
    let mut out = Vec::new();
    let mut i = 0;
    let mut start: Option<usize> = None;
    while i + 3 <= au.len() {
        let three = au[i] == 0 && au[i + 1] == 0 && au[i + 2] == 1;
        if three {
            if let Some(s) = start {
                let end = i;
                if end > s {
                    out.push((au[s] & 0x1f, &au[s + 1..end]));
                }
            }
            // `00 00 01` is found at the same relative place in a three- and a
            // four-byte start code, and the NAL header byte follows it either
            // way. Trailing zeros of the previous payload are start-code bytes;
            // the parser never reads the tail, so they are left alone.
            start = Some(i + 3);
            i += 3;
            continue;
        }
        i += 1;
    }
    if let Some(s) = start {
        if s < au.len() {
            out.push((au[s] & 0x1f, &au[s + 1..]));
        }
    }
    out
}

/// RBSP bit reader: skips the emulation-prevention byte, as the slice header is
/// read as RBSP, not as the raw byte stream.
struct Bits<'a> {
    data: &'a [u8],
    byte: usize,
    bit: u8,
    zeros: u8,
}

impl<'a> Bits<'a> {
    fn new(data: &'a [u8]) -> Self {
        Self { data, byte: 0, bit: 0, zeros: 0 }
    }

    /// Next RBSP bit, or `None` at the end of the buffer.
    fn next_bit(&mut self) -> Option<u32> {
        loop {
            if self.byte >= self.data.len() {
                return None;
            }
            let b = self.data[self.byte];
            // Inside the RBSP, 0x03 after two zero bytes is a stuffing byte.
            if self.bit == 0 && self.zeros >= 2 && b == 0x03 {
                self.byte += 1;
                self.zeros = 0;
                continue;
            }
            let v = (b >> (7 - self.bit)) & 1;
            self.bit += 1;
            if self.bit == 8 {
                self.bit = 0;
                self.byte += 1;
            }
            self.zeros = if v == 0 { self.zeros.saturating_add(1) } else { 0 };
            return Some(u32::from(v));
        }
    }

    fn read_bits(&mut self, n: u8) -> Option<u32> {
        let mut v = 0u32;
        for _ in 0..n {
            v = (v << 1) | self.next_bit()?;
        }
        Some(v)
    }

    /// Unsigned Exp-Golomb, with a bound so a corrupt stream cannot spin.
    fn read_ue(&mut self) -> Option<u32> {
        let mut zeros = 0u8;
        while self.next_bit()? == 0 {
            zeros += 1;
            if zeros > 31 {
                return None;
            }
        }
        let rest = if zeros == 0 { 0 } else { self.read_bits(zeros)? };
        Some((1u32 << zeros) - 1 + rest)
    }
}

/// The facts the cheap path can supply, or a refusal.
fn classify_slice(payload: &[u8], sps: &SpsBits) -> Option<(bool, u16)> {
    let width = sps.log2_max_frame_num_minus4 as u32 + 4;
    if width > 16 {
        return None;
    }
    let mut b = Bits::new(payload);
    let _first_mb_in_slice = b.read_ue()?;
    let slice_type = b.read_ue()?;
    let _pic_parameter_set_id = b.read_ue()?;
    if sps.separate_colour_plane {
        let _colour_plane_id = b.read_bits(2)?;
    }
    let frame_num = b.read_bits(width as u8)?;
    // slice_type is coded modulo 5: 0 P, 1 B, 2 I, 3 SP, 4 SI.
    Some((slice_type % 5 == 1, frame_num as u16))
}

/// Classify one Annex-B access unit.
pub(crate) fn classify(au: &[u8], sps: &SpsBits) -> AuClass {
    let mut has_b_slice = false;
    let mut frame_num: Option<u16> = None;
    let mut saw_vcl = false;
    for (kind, payload) in nal_units(au) {
        match kind {
            NAL_SLICE => {
                saw_vcl = true;
                match classify_slice(payload, sps) {
                    Some((is_b, num)) => {
                        has_b_slice |= is_b;
                        // The picture's slices share one frame_num; a disagreement
                        // means this AU is not the simple shape the cheap path
                        // assumes.
                        match frame_num {
                            None => frame_num = Some(num),
                            Some(seen) if seen == num => {}
                            Some(_) => return AuClass::NeedsFullPlan,
                        }
                    }
                    None => return AuClass::NeedsFullPlan,
                }
            }
            NAL_IDR | NAL_SEI | NAL_SPS | NAL_PPS => return AuClass::NeedsFullPlan,
            NAL_AUD | NAL_END_SEQ | NAL_END_STREAM | NAL_FILLER => {}
            // Partition NALs, SPS/PPS extensions, auxiliary and depth slices: no
            // cheap parse exists, and the planner knows what they mean.
            _ => return AuClass::NeedsFullPlan,
        }
    }
    match (saw_vcl, frame_num) {
        (true, Some(num)) => AuClass::Plain { has_b_slice, frame_num: num },
        _ => AuClass::NeedsFullPlan,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Emit one Annex-B NAL with a 4-byte start code.
    fn nal(kind: u8, payload: &[u8]) -> Vec<u8> {
        let mut v = vec![0, 0, 0, 1, kind & 0x1f];
        v.extend_from_slice(payload);
        v
    }

    /// Bits -> bytes, MSB first, with emulation prevention applied.
    struct W(Vec<u8>, u8);
    impl W {
        fn new() -> Self {
            W(Vec::new(), 0)
        }
        fn bit(&mut self, b: u32) {
            if self.1 == 0 {
                self.0.push(0);
            }
            let last = self.0.len() - 1;
            self.0[last] |= ((b & 1) as u8) << (7 - self.1);
            self.1 = (self.1 + 1) % 8;
        }
        fn bits(&mut self, v: u32, n: u8) {
            for i in (0..n).rev() {
                self.bit((v >> i) & 1);
            }
        }
        fn ue(&mut self, v: u32) {
            let n = 32 - (v + 1).leading_zeros();
            for _ in 0..n - 1 {
                self.bit(0);
            }
            self.bits(v + 1, n as u8);
        }
        fn finish(mut self) -> Vec<u8> {
            if self.1 != 0 {
                self.1 = 0;
            }
            // Apply emulation prevention so the reader has to strip it.
            let mut out = Vec::new();
            let mut zeros = 0;
            for b in self.0 {
                if zeros >= 2 && b <= 3 {
                    out.push(3);
                    zeros = 0;
                }
                zeros = if b == 0 { zeros + 1 } else { 0 };
                out.push(b);
            }
            out
        }
    }

    fn slice_payload(slice_type: u32, frame_num: u32, width: u8) -> Vec<u8> {
        let mut w = W::new();
        w.ue(0); // first_mb_in_slice
        w.ue(slice_type);
        w.ue(0); // pic_parameter_set_id
        w.bits(frame_num, width);
        w.bits(0b1010_1010, 8); // trailing slice data, unread
        w.finish()
    }

    fn sps(log2_minus4: u8) -> SpsBits {
        SpsBits { log2_max_frame_num_minus4: log2_minus4, separate_colour_plane: false }
    }

    #[test]
    fn a_plain_p_au_yields_its_frame_number() {
        let au = nal(NAL_SLICE, &slice_payload(0, 1234, 16));
        assert_eq!(classify(&au, &sps(12)), AuClass::Plain { has_b_slice: false, frame_num: 1234 });
    }

    #[test]
    fn a_b_slice_is_reported_so_the_gate_can_refuse() {
        let au = nal(NAL_SLICE, &slice_payload(1, 7, 8));
        assert_eq!(classify(&au, &sps(4)), AuClass::Plain { has_b_slice: true, frame_num: 7 });
        // slice_type 6 is B in the +5 encoding.
        let au = nal(NAL_SLICE, &slice_payload(6, 7, 8));
        assert_eq!(classify(&au, &sps(4)), AuClass::Plain { has_b_slice: true, frame_num: 7 });
    }

    #[test]
    fn state_carrying_nals_always_ask_for_the_full_planner() {
        for kind in [NAL_IDR, NAL_SEI, NAL_SPS, NAL_PPS, 2, 3, 4, 13, 19, 20, 21] {
            let au = nal(kind, &slice_payload(0, 1, 8));
            assert_eq!(classify(&au, &sps(4)), AuClass::NeedsFullPlan, "nal {kind}");
        }
    }

    #[test]
    fn an_au_without_a_vcl_nal_asks_for_the_full_planner() {
        let mut au = nal(NAL_AUD, &[0x10]);
        au.extend_from_slice(&nal(NAL_FILLER, &[0xff, 0xff]));
        assert_eq!(classify(&au, &sps(4)), AuClass::NeedsFullPlan);
    }

    #[test]
    fn emulation_prevention_inside_the_header_is_stripped() {
        // A frame_num of 0x0003 in a 16-bit field forces a stuffing byte, which
        // must not be read as data.
        let au = nal(NAL_SLICE, &slice_payload(0, 0x0003, 16));
        assert_eq!(classify(&au, &sps(12)), AuClass::Plain { has_b_slice: false, frame_num: 3 });
    }

    #[test]
    fn slices_disagreeing_on_frame_num_ask_for_the_full_planner() {
        let mut au = nal(NAL_SLICE, &slice_payload(0, 5, 8));
        au.extend_from_slice(&nal(NAL_SLICE, &slice_payload(0, 6, 8)));
        assert_eq!(classify(&au, &sps(4)), AuClass::NeedsFullPlan);
    }

    #[test]
    fn several_slices_of_one_picture_are_accepted() {
        let mut au = nal(NAL_SLICE, &slice_payload(0, 9, 8));
        au.extend_from_slice(&nal(NAL_SLICE, &slice_payload(0, 9, 8)));
        au.extend_from_slice(&nal(NAL_AUD, &[0x10]));
        assert_eq!(classify(&au, &sps(4)), AuClass::Plain { has_b_slice: false, frame_num: 9 });
    }

    #[test]
    fn a_truncated_header_is_refused_rather_than_guessed() {
        let au = nal(NAL_SLICE, &[0xff]);
        assert_eq!(classify(&au, &sps(12)), AuClass::NeedsFullPlan);
    }

    #[test]
    fn a_frame_num_wider_than_sixteen_bits_is_refused() {
        let au = nal(NAL_SLICE, &slice_payload(0, 1, 16));
        assert_eq!(classify(&au, &sps(13)), AuClass::NeedsFullPlan);
    }

    #[test]
    fn an_empty_au_is_refused() {
        assert_eq!(classify(&[], &sps(4)), AuClass::NeedsFullPlan);
    }
}
