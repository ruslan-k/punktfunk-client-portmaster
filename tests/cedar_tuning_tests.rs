#[path = "../patches/cedar_tuning.rs"]
mod tuning;
use tuning::CedarTuning;
#[test]
fn missing_options_preserve_verified_vendor_baseline() {
    let t=CedarTuning::from_lookup(|_|None).unwrap();
    assert_eq!((t.no_b_frames,t.frame_package,t.smooth,t.display,t.drop_b_delay),(0,0,2,2,0));
}
#[test]
fn each_candidate_changes_only_its_single_axis() {
    for (name,value,expected) in [
        ("PUNKTFUNK_CEDAR_NO_B","1",(1,0,2,2,0)),
        ("PUNKTFUNK_CEDAR_FRAME_PACKAGE","1",(0,1,2,2,0)),
        ("PUNKTFUNK_CEDAR_SMOOTH","1",(0,0,1,2,0)),
        ("PUNKTFUNK_CEDAR_DISPLAY","1",(0,0,2,1,0)),
        ("PUNKTFUNK_CEDAR_DROP_B_DELAY","1",(0,0,2,2,1)),
    ] {
        let t=CedarTuning::from_lookup(|k|if k==name {Some(value.into())} else {None}).unwrap();
        assert_eq!((t.no_b_frames,t.frame_package,t.smooth,t.display,t.drop_b_delay),expected);
    }
}
#[test]
fn unsafe_or_malformed_candidate_is_refused() {
    for (name,value) in [("PUNKTFUNK_CEDAR_NO_B","2"),("PUNKTFUNK_CEDAR_SMOOTH","0"),("PUNKTFUNK_CEDAR_DISPLAY","-1"),("PUNKTFUNK_CEDAR_SMOOTH","99"),("PUNKTFUNK_CEDAR_FRAME_PACKAGE","true")] {
        assert!(CedarTuning::from_lookup(|k|if k==name {Some(value.into())} else {None}).is_err());
    }
}
