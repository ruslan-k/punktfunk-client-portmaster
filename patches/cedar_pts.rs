//! Bounded exact-PTS association; never guesses by slot or FIFO position.
//!
//! The window is a fixed array of [`PTS_SLOTS`] slots: a submit writes one slot, a take
//! scans the live entries, and neither allocates. The `BTreeMap` this replaced
//! allocated a node per submit and freed it per take - one malloc/free pair per frame,
//! in the drain's hot path, on a device where the tail latency is what matters.

/// The historical outstanding limit; also the size of the fixed window.
const PTS_SLOTS: usize = 128;

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
pub(crate) struct PtsLedger<T> {
    /// Live entries packed into `slots[..len]`; the rest is never touched. Dense on
    /// purpose: a sparse window made every lookup walk all [`PTS_SLOTS`] slots, which
    /// measured +0.07 ms per frame (14 KB of cold cache lines per frame) against the
    /// BTreeMap it replaced. Order is irrelevant to every caller, so a take swaps the
    /// last live entry into the hole.
    slots: [Option<(i64, T)>; PTS_SLOTS],
    len: usize,
}
impl<T> Default for PtsLedger<T> {
    fn default() -> Self { Self { slots: std::array::from_fn(|_| None), len: 0 } }
}
impl<T> PtsLedger<T> {
    pub fn insert(&mut self, pts: i64, facts: T) -> Result<(), &'static str> {
        if pts <= 0 { return Err("nonpositive Cedar PTS"); }
        if self.live().any(|(k, _)| *k == pts) { return Err("duplicate Cedar PTS"); }
        if self.len >= PTS_SLOTS { return Err("128 outstanding Cedar PTS limit reached"); }
        // `len` is the next free slot: the window is packed, so this is O(1).
        self.slots[self.len] = Some((pts, facts));
        self.len += 1;
        Ok(())
    }
    pub fn take(&mut self, pts: i64) -> Option<T> {
        let idx = self.live().position(|(k, _)| *k == pts)?;
        let (_, facts) = self.slots[idx].take()?;
        self.len -= 1;
        // Move the last live entry into the hole; nothing depends on the order.
        let last = self.slots[self.len].take();
        self.slots[idx] = last;
        Some(facts)
    }
    pub fn len(&self) -> usize { self.len }
    /// The live entries only - never the free tail of the window.
    fn live(&self) -> impl Iterator<Item = &(i64, T)> {
        self.slots[..self.len].iter().filter_map(|s| s.as_ref())
    }
}
