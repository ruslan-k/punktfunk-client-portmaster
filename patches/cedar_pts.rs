//! Bounded exact-PTS association; never guesses by slot or FIFO position.
use std::collections::BTreeMap;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct FrameStamp {
    pub received_ns: u64,
    pub pts_ns: u64,
    pub flags: u32,
    pub frame_index: u32,
    /// Client wall clock when pixels finished copying; 0 on input.
    pub ready_ns: u64,
}
#[derive(Default)]
pub(crate) struct TokenClock { last: i64 }
impl TokenClock {
    pub fn next(&mut self, pts_ns: u64) -> Option<i64> {
        let preferred = i64::try_from(pts_ns / 1000).ok()?.max(1);
        self.last = preferred.max(self.last.checked_add(1)?);
        Some(self.last)
    }
}
pub(crate) struct PtsLedger<T> { entries: BTreeMap<i64, T> }
impl<T> Default for PtsLedger<T> {
    fn default() -> Self { Self { entries: BTreeMap::new() } }
}
impl<T> PtsLedger<T> {
    pub fn insert(&mut self, pts: i64, facts: T) -> Result<(), &'static str> {
        if pts <= 0 { return Err("nonpositive Cedar PTS"); }
        if self.entries.contains_key(&pts) { return Err("duplicate Cedar PTS"); }
        if self.entries.len() >= 128 { return Err("128 outstanding Cedar PTS limit reached"); }
        self.entries.insert(pts, facts);
        Ok(())
    }
    pub fn take(&mut self, pts: i64) -> Option<T> { self.entries.remove(&pts) }
    pub fn len(&self) -> usize { self.entries.len() }
}
