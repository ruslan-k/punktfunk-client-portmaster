pub(crate) const AUD_DELIMITER:&[u8]=&[0,0,0,1,9,0xf0];
pub(crate) const AUD_PTS:i64=-1;
pub(crate) fn retry_async(rc:i32,pictures:usize,elapsed_us:u64,budget_us:u64)->bool {
    budget_us>0 && pictures==0 && matches!(rc,2|5) && elapsed_us<budget_us
}
