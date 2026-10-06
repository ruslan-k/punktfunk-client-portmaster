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
fn async_controls_are_opt_in_and_bounded() {
    let baseline=CedarTuning::from_lookup(|_|None).unwrap();
    assert_eq!((baseline.poll_budget_us,baseline.append_aud,baseline.low_delay),(0,0,0));
    let low=CedarTuning::from_lookup(|k|if k=="PUNKTFUNK_CEDAR_LOW_DELAY" {Some("1".into())} else {None}).unwrap();
    assert_eq!((low.low_delay,low.poll_budget_us,low.no_b_frames),(1,0,0));
    let auto=CedarTuning::from_lookup(|k|if k=="PUNKTFUNK_CEDAR_LOW_DELAY" {Some("auto".into())} else {None}).unwrap();
    assert_eq!(auto.low_delay,-1);
    assert!(CedarTuning::from_lookup(|k|if k=="PUNKTFUNK_CEDAR_LOW_DELAY" {Some("2".into())} else {None}).is_err());
    let t=CedarTuning::from_lookup(|k|match k { "PUNKTFUNK_CEDAR_POLL_US"=>Some("5000".into()),"PUNKTFUNK_CEDAR_AUD"=>Some("1".into()),_=>None }).unwrap();
    assert_eq!((t.poll_budget_us,t.append_aud),(5000,1));
    assert!(CedarTuning::from_lookup(|k|if k=="PUNKTFUNK_CEDAR_POLL_US" {Some("20000".into())} else {None}).is_err());
}
#[test]
fn unsafe_or_malformed_candidate_is_refused() {
    for (name,value) in [("PUNKTFUNK_CEDAR_NO_B","2"),("PUNKTFUNK_CEDAR_SMOOTH","0"),("PUNKTFUNK_CEDAR_DISPLAY","-1"),("PUNKTFUNK_CEDAR_SMOOTH","99"),("PUNKTFUNK_CEDAR_FRAME_PACKAGE","true")] {
        assert!(CedarTuning::from_lookup(|k|if k==name {Some(value.into())} else {None}).is_err());
    }
}
