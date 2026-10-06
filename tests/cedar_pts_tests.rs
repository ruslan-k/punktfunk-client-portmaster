#[path = "../patches/cedar_pts.rs"]
mod pts;
use pts::PtsLedger;

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
