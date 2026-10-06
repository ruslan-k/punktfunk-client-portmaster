#[path = "../patches/cedar_async.rs"]
mod pacing;
#[test]
fn retry_is_bounded_and_only_for_empty_async_parser_results() {
    assert!(pacing::retry_async(5,0,20,5000));
    assert!(pacing::retry_async(2,0,20,5000));
    assert!(!pacing::retry_async(5,0,5000,5000));
    assert!(!pacing::retry_async(5,1,20,5000));
    assert!(!pacing::retry_async(4,0,20,5000));
    assert!(!pacing::retry_async(5,0,0,0));
}
#[test]
fn delimiter_is_non_vcl_and_has_no_picture_identity() {
    assert_eq!(pacing::AUD_DELIMITER, &[0,0,0,1,9,0xf0]);
    assert_eq!(pacing::AUD_PTS,-1);
    assert_eq!(pacing::AUD_DELIMITER[4]&0x1f,9);
}
