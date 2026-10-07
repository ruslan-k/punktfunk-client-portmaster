//! Opt-in, bounded Cedar phase counters; no per-frame logging or payloads.
use std::time::Instant;
use std::collections::VecDeque;
pub(crate) fn poll_picture<T>(pending: &mut VecDeque<T>) -> Option<T> { pending.pop_front() }
pub(crate) fn note_lag(hist: &mut [u64; 5], input: u64, source: u64) {
    hist[input.saturating_sub(source).min(4) as usize] += 1;
}

/// Keep all decoded pictures in FIFO mode; retain the exact original policy for A/B.
pub(crate) fn queue_picture<T>(pending: &mut VecDeque<T>, newest: &mut Option<T>,
    frame: T, fifo: bool) -> bool {
    if fifo { pending.push_back(frame); false } else { newest.replace(frame).is_some() }
}

/// Stage indices: 0 planner, 1 feed AU, 2 whole `decode` entry (contains 0 and
/// 1), 3 `RequestPicture`, 4 picture copy, 5 `ReturnPicture`, 6 the drain loop's
/// own body (FIFO, ledger, bookkeeping), 7 the whole `drain` call, 8 the frame
/// arm (contains 3..5) and 9 the non-frame arms (the retry decision). 10 is the
/// retry backoff sleep: it sits inside 6 but outside 8 and 9, which is why it
/// looked like unattributed body time. 2, 6, 7 and 8 overlap their sub-stages, so
/// the columns must be subtracted, never summed.
#[derive(Default, Debug)]
pub(crate) struct PhaseStats {
    pub stage_n: [u64; 11],
    pub stage_us: [u64; 11],
    pub stage_max_us: [u64; 11],
    pub vendor_n: [u64; 8],
    pub vendor_us: [u64; 8],
    pub vendor_max_us: [u64; 8],
    pub drain_pictures: [u64; 4],
    pub pictures: u64,
    pub empty_pictures: u64,
    pub replaced_pictures: u64,
}

pub(crate) fn elapsed_us(start: Option<Instant>) -> u64 {
    start.map_or(0, |s| s.elapsed().as_micros() as u64)
}

impl PhaseStats {
    // Stage indices: 0 plan, 1 feed, 2 whole decode call, 3 RequestPicture,
    // 4 picture copy, 5 ReturnPicture. Whole-call time overlaps subphases.
    pub fn note_stage(&mut self, stage: usize, us: u64) {
        self.stage_n[stage] += 1;
        self.stage_us[stage] += us;
        self.stage_max_us[stage] = self.stage_max_us[stage].max(us);
    }
    pub fn note_vendor(&mut self, rc: i32, us: u64) {
        let index = if (0..=6).contains(&rc) { rc as usize } else { 7 };
        self.vendor_n[index] += 1;
        self.vendor_us[index] += us;
        self.vendor_max_us[index] = self.vendor_max_us[index].max(us);
    }
    pub fn note_drain(&mut self, count: usize) {
        self.drain_pictures[count.min(3)] += 1;
    }
    pub fn note_copy(&mut self, us: u64) {
        self.pictures += 1;
        self.note_stage(4, us);
    }
    pub fn json(&self, au: u64, frames: u64) -> String {
        format!(concat!("{{\"au\":{},\"frames\":{},\"stage_n\":{:?},",
            "\"stage_us\":{:?},\"stage_max_us\":{:?},\"vendor_n\":{:?},",
            "\"vendor_us\":{:?},\"vendor_max_us\":{:?},\"pictures\":{},",
            "\"empty_pictures\":{},\"replaced_pictures\":{},\"drain_pictures\":{:?}}}"),
            au, frames, self.stage_n, self.stage_us, self.stage_max_us,
            self.vendor_n, self.vendor_us, self.vendor_max_us,
            self.pictures, self.empty_pictures, self.replaced_pictures, self.drain_pictures)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ready_burst_can_be_handed_on_without_another_network_au() {
        let mut queue = VecDeque::from([(10, 1000), (11, 2000)]);
        assert_eq!(poll_picture(&mut queue), Some((10, 1000)));
        assert_eq!(poll_picture(&mut queue), Some((11, 2000)));
        assert_eq!(poll_picture(&mut queue), None);
        assert!(queue.is_empty());
    }
    #[test]
    fn lag_histogram_counts_exact_source_au_distance() {
        let mut hist = [0; 5];
        for source in [100, 99, 98, 97, 96, 80] { note_lag(&mut hist, 100, source); }
        assert_eq!(hist, [1, 1, 1, 1, 2]);
    }
    #[test]
    fn fifo_burst_preserves_all_pictures_and_their_order() {
        let mut queue = VecDeque::new();
        let mut newest = None;
        for frame in [10, 11, 12] {
            assert!(!queue_picture(&mut queue, &mut newest, frame, true));
        }
        assert!(newest.is_none());
        assert_eq!(queue.pop_front(), Some(10));
        assert_eq!(queue.pop_front(), Some(11));
        assert_eq!(queue.pop_front(), Some(12));
        assert!(queue.is_empty());
    }
    #[test]
    fn baseline_newest_wins_is_retained_for_control_runs() {
        let mut queue = VecDeque::new();
        let mut newest = None;
        assert!(!queue_picture(&mut queue, &mut newest, 10, false));
        assert!(queue_picture(&mut queue, &mut newest, 11, false));
        assert_eq!(newest, Some(11));
        assert!(queue.is_empty());
    }
    #[test]
    fn counts_and_durations_are_kept_per_return_code() {
        let mut p = PhaseStats::default();
        p.note_vendor(1, 2000);
        p.note_vendor(5, 10000);
        p.note_vendor(5, 11000);
        p.note_vendor(-1, 50);
        assert_eq!(p.vendor_n, [0,1,0,0,0,2,0,1]);
        assert_eq!(p.vendor_us[5], 21000);
        assert_eq!(p.vendor_max_us[5], 11000);
    }
    #[test]
    fn copy_and_empty_picture_counts_are_independent() {
        let mut p = PhaseStats::default();
        p.note_drain(0);
        p.note_drain(1);
        p.note_drain(2);
        p.note_drain(5);
        assert_eq!(p.drain_pictures, [1,1,1,1]);
        p.note_copy(100);
        p.note_copy(200);
        p.empty_pictures += 1;
        assert_eq!(p.pictures, 2);
        assert_eq!(p.stage_n[4], 2);
        assert_eq!(p.stage_us[4], 300);
        assert_eq!(p.empty_pictures, 1);
        assert!(p.json(3, 2).contains("\"pictures\":2"));
    }
    #[test]
    fn resetting_a_window_does_not_retain_prior_samples() {
        let mut p = PhaseStats::default();
        p.note_vendor(1, 5);
        let old = std::mem::take(&mut p);
        assert_eq!(old.vendor_n[1], 1);
        assert_eq!(p.vendor_n, [0;8]);
        assert_eq!(elapsed_us(None), 0);
    }
}
