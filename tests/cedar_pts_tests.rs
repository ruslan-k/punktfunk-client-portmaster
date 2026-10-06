#[path = "../patches/cedar_pts.rs"]
mod pts;
use pts::{PtsLedger, TokenClock};

#[test]
fn token_clock_uses_capture_microseconds_and_disambiguates_repeats() {
    let mut t = TokenClock::default();
    assert_eq!(t.next(100000000), Some(100000));
    assert_eq!(t.next(100000000), Some(100001));
    assert_eq!(t.next(99999000), Some(100002));
    assert_eq!(t.next(200000000), Some(200000));
}
#[test]
fn zero_capture_stamp_never_creates_a_nonpositive_token() {
    let mut t = TokenClock::default();
    assert_eq!(t.next(0), Some(1));
    assert_eq!(t.next(0), Some(2));
}


#[test]
fn transport_flags_and_colour_stay_with_their_exact_original_token() {
    use pts::FrameStamp;
    let first = FrameStamp { received_ns: 100, pts_ns: 90, flags: 1, frame_index: 10, ready_ns: 0 };
    let newer = FrameStamp { received_ns: 200, pts_ns: 190, flags: 2, frame_index: 11, ready_ns: 0 };
    let mut ledger = PtsLedger::default();
    ledger.insert(1000, (first, true, 7u8, 1u8)).unwrap();
    ledger.insert(2000, (newer, false, 8u8, 2u8)).unwrap();
    assert_eq!(ledger.take(1000), Some((first, true, 7, 1)));
    assert_eq!(ledger.take(2000), Some((newer, false, 8, 2)));
}

#[test]
fn exact_tokens_survive_reordered_vendor_output() {
    let mut p = PtsLedger::default();
    p.insert(100000, "AU100").unwrap();
    p.insert(101000, "AU101").unwrap();
    p.insert(102000, "AU102").unwrap();
    assert_eq!(p.take(101000), Some("AU101"));
    assert_eq!(p.take(100000), Some("AU100"));
    assert_eq!(p.take(102000), Some("AU102"));
    assert_eq!(p.len(), 0);
}
#[test]
fn unknown_and_duplicate_output_do_not_consume_another_au() {
    let mut p = PtsLedger::default();
    p.insert(100000, 100).unwrap();
    p.insert(101000, 101).unwrap();
    assert_eq!(p.take(-1), None);
    assert_eq!(p.take(100001), None);
    assert_eq!(p.len(), 2);
    assert_eq!(p.take(101000), Some(101));
    assert_eq!(p.take(101000), None);
    assert_eq!(p.take(100000), Some(100));
}
#[test]
fn duplicate_or_nonpositive_submission_is_refused_without_overwrite() {
    let mut p = PtsLedger::default();
    assert!(p.insert(-1, 7).is_err());
    assert!(p.insert(0, 7).is_err());
    p.insert(1000, 42).unwrap();
    assert!(p.insert(1000, 999).is_err());
    assert_eq!(p.take(1000), Some(42));
}
#[test]
fn bounded_ledger_refuses_growth_and_reuses_retired_capacity() {
    let mut p = PtsLedger::default();
    for i in 1..=128 { p.insert(i, i).unwrap(); }
    assert!(p.insert(129, 129).is_err());
    assert_eq!(p.len(), 128);
    assert_eq!(p.take(1), Some(1));
    p.insert(129, 129).unwrap();
    assert_eq!(p.take(129), Some(129));
}
