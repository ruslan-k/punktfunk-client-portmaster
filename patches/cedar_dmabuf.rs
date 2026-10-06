//! Opt-in read-only probe: does the vendor picture's dma-buf hold the same bytes
//! the CPU copy produced, at the offsets a zero-copy import would use?
//!
//! The device exports one linear 1382400-byte frame per picture slot plus 2 KiB
//! of padding (`dma_buf/bufinfo` shows 14 buffers of 1384448 bytes attached to
//! `1c0e000.ve`). Before any presenter import, prove the plane placement instead
//! of assuming it: a wrong offset is a wrong picture, not a crash.

/// Bytes of luma for `width` x `height` 4:2:0.
pub(crate) fn luma_len(width: u32, height: u32) -> Option<usize> {
    (width as usize).checked_mul(height as usize)
}

/// Bytes of one chroma plane for `width` x `height` 4:2:0.
pub(crate) fn chroma_len(width: u32, height: u32) -> Option<usize> {
    let (w, h) = ((width / 2) as usize, (height / 2) as usize);
    w.checked_mul(h)
}

/// Tightly packed `YV12` (Y, then V, then U) offsets, the layout the vendor
/// reports with `e_pixel_format = 4` and `lineStride == width`.
pub(crate) fn yv12_offsets(width: u32, height: u32) -> Option<(usize, usize, usize)> {
    let y = luma_len(width, height)?;
    let c = chroma_len(width, height)?;
    let v = y;
    let u = y.checked_add(c)?;
    Some((0, v, u))
}

/// Total bytes such a frame occupies.
pub(crate) fn packed_len(width: u32, height: u32) -> Option<usize> {
    luma_len(width, height)?.checked_add(chroma_len(width, height)?.checked_mul(2)?)
}

/// True when `blob` carries exactly these three already-packed planes at the
/// given offsets. Chroma order is the caller's contract: `v_plane` is what the
/// vendor wrote first.
pub(crate) fn matches(
    blob: &[u8],
    offsets: (usize, usize, usize),
    y_plane: &[u8],
    v_plane: &[u8],
    u_plane: &[u8],
) -> bool {
    let (y_off, v_off, u_off) = offsets;
    let region = |off: usize, len: usize| blob.get(off..off.checked_add(len)?);
    region(y_off, y_plane.len()) == Some(y_plane)
        && region(v_off, v_plane.len()) == Some(v_plane)
        && region(u_off, u_plane.len()) == Some(u_plane)
}

/// First offset where `blob` differs from a packed `YV12` reference, for a
/// one-line diagnostic when `matches` is false.
pub(crate) fn first_difference(
    blob: &[u8],
    offsets: (usize, usize, usize),
    y_plane: &[u8],
    v_plane: &[u8],
    u_plane: &[u8],
) -> Option<usize> {
    let (y_off, v_off, u_off) = offsets;
    for (off, plane) in [(y_off, y_plane), (v_off, v_plane), (u_off, u_plane)] {
        let len = plane.len();
        for i in 0..len {
            match blob.get(off + i) {
                Some(b) if *b == plane[i] => {}
                _ => return Some(off + i),
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn synthetic(width: u32, height: u32) -> (Vec<u8>, (Vec<u8>, Vec<u8>, Vec<u8>)) {
        let y = luma_len(width, height).unwrap();
        let c = chroma_len(width, height).unwrap();
        let (y_off, v_off, u_off) = yv12_offsets(width, height).unwrap();
        let mut blob = vec![0xEEu8; packed_len(width, height).unwrap()];
        let luma: Vec<u8> = (0..y).map(|i| (i % 251) as u8).collect();
        let vp: Vec<u8> = (0..c).map(|i| (i % 241) as u8).collect();
        let up: Vec<u8> = (0..c).map(|i| (i % 239) as u8).collect();
        blob[y_off..y_off + y].copy_from_slice(&luma);
        blob[v_off..v_off + c].copy_from_slice(&vp);
        blob[u_off..u_off + c].copy_from_slice(&up);
        (blob, (luma, vp, up))
    }

    #[test]
    fn standard_yv12_layout_matches_its_own_reference() {
        let (blob, (y, v, u)) = synthetic(1280, 720);
        let off = yv12_offsets(1280, 720).unwrap();
        assert_eq!(off, (0, 921_600, 1_152_000));
        assert_eq!(packed_len(1280, 720).unwrap(), 1_382_400);
        assert!(matches(&blob, off, &y, &v, &u));
        assert_eq!(first_difference(&blob, off, &y, &v, &u), None);
    }

    #[test]
    fn swapped_chroma_or_a_shifted_base_is_detected() {
        let (blob, (y, v, u)) = synthetic(1280, 720);
        let off = yv12_offsets(1280, 720).unwrap();
        assert!(!matches(&blob, off, &y, &u, &v), "swapped planes must not match");
        assert_eq!(first_difference(&blob, off, &y, &u, &v), Some(921_600));
        let shifted = (2048, 2048 + 921_600, 2048 + 1_152_000);
        assert!(!matches(&blob, shifted, &y, &v, &u), "a leading pad must not match");
        assert_eq!(first_difference(&blob, shifted, &y, &v, &u), Some(2048),
            "the first luma byte already differs from a padded base");
    }

    #[test]
    fn short_or_truncated_blobs_are_refused_not_read() {
        let (blob, (y, v, u)) = synthetic(64, 64);
        let off = yv12_offsets(64, 64).unwrap();
        assert!(matches(&blob, off, &y, &v, &u));
        assert!(!matches(&blob[..blob.len() - 1], off, &y, &v, &u));
        assert_eq!(first_difference(&blob[..blob.len() - 1], off, &y, &v, &u), Some(blob.len() - 1));
        assert!(yv12_offsets(0, 720).is_some(), "zero width stays arithmetic, not a panic");
    }
}
