//! Bounded exact-PTS association; never guesses by slot or FIFO position.
use std::collections::BTreeMap;
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
