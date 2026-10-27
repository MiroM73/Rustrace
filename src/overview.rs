//! Overview strip next to the table's scroll bar (#24): the highest mark (error, search match,
//! …) of the displayed rows, kept in at most `MAX_BUCKETS` buckets of `per` rows. After a
//! re-filter only the buckets from the first changed row on are computed again (appended rows:
//! just the end); drawing reads a few buckets per pixel.

/// Most buckets kept; the rows per bucket double when there are more rows.
const MAX_BUCKETS: usize = 4096;

#[derive(Default, Debug)]
pub struct Overview {
    /// Displayed rows per bucket (a power of two).
    per: usize,
    /// Highest mark of the rows of every bucket.
    buckets: Vec<u8>,
    /// Displayed rows covered.
    len: usize,
}

impl Overview {
    /// Rows covered by the last update.
    pub fn len(&self) -> usize {
        self.len
    }

    /// Computes the marks again from row `from` on, for `len` displayed rows; `mark(row)` gives
    /// the mark of one displayed row (0 = none, higher wins).
    pub fn update(&mut self, from: usize, len: usize, mark: impl Fn(usize) -> u8) {
        let mut from = from.min(self.len).min(len);
        if from == 0 || self.per == 0 {
            self.per = 1;
            self.buckets.clear();
            from = 0;
        }
        while len.div_ceil(self.per) > MAX_BUCKETS {
            // Twice the rows per bucket: pairs of buckets are joined.
            self.per *= 2;
            self.buckets = self
                .buckets
                .chunks(2)
                .map(|c| c.iter().copied().max().unwrap_or(0))
                .collect();
        }
        // The bucket with `from` (and any after it) is computed again.
        self.buckets.truncate(from / self.per);
        for b in self.buckets.len()..len.div_ceil(self.per) {
            let rows = b * self.per..((b + 1) * self.per).min(len);
            self.buckets.push(rows.map(&mark).max().unwrap_or(0));
        }
        self.len = len;
    }

    /// Highest mark of the displayed rows `a..b`.
    pub fn mark_range(&self, a: usize, b: usize) -> u8 {
        if self.per == 0 || a >= b || a >= self.len {
            return 0;
        }
        let last = (b.min(self.len) - 1) / self.per;
        self.buckets[a / self.per..=last]
            .iter()
            .copied()
            .max()
            .unwrap_or(0)
    }

    /// Marks per pixel row of a strip `height` pixels high, joined into runs:
    /// (first pixel, pixel count, mark), marks > 0 only.
    pub fn runs(&self, height: usize) -> Vec<(usize, usize, u8)> {
        let mut runs: Vec<(usize, usize, u8)> = Vec::new();
        if self.len == 0 || height == 0 {
            return runs;
        }
        for y in 0..height {
            let a = y * self.len / height;
            let b = ((y + 1) * self.len / height).max(a + 1);
            let m = self.mark_range(a, b);
            if m == 0 {
                continue;
            }
            match runs.last_mut() {
                Some((start, n, last)) if *last == m && *start + *n == y => *n += 1,
                _ => runs.push((y, 1, m)),
            }
        }
        runs
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn marks(n: usize, seed: usize) -> Vec<u8> {
        (0..n)
            .map(|i| match (i * 7919 + seed) % 1000 {
                0..=2 => 3,
                3..=20 => 2,
                21..=60 => 1,
                _ => 0,
            })
            .collect()
    }

    fn check(o: &Overview, m: &[u8]) {
        assert_eq!(o.len(), m.len());
        for (a, b) in [
            (0, 1),
            (0, m.len()),
            (5, 9000),
            (12_345, 12_346),
            (100, 70_000),
        ] {
            let b = b.min(m.len());
            let want = m
                .get(a..b)
                .map_or(0, |s| s.iter().copied().max().unwrap_or(0));
            let got = o.mark_range(a, b);
            // A bucket can reach a little past the range, never miss a mark inside it.
            assert!(got >= want, "{a}..{b}");
            if o.per == 1 {
                assert_eq!(got, want);
            }
        }
    }

    #[test]
    fn incremental_equals_full() {
        let all = marks(100_000, 1);
        // Appended in pieces (auto refresh) = computed at once.
        let mut inc = Overview::default();
        for end in [0, 1, 100, 4096, 4097, 9000, 30_000, 100_000] {
            inc.update(inc.len(), end, |r| all[r]);
            check(&inc, &all[..end]);
        }
        let mut full = Overview::default();
        full.update(0, all.len(), |r| all[r]);
        assert_eq!((inc.per, &inc.buckets), (full.per, &full.buckets));
        assert!(full.buckets.len() <= MAX_BUCKETS);

        // The end changed (rows merged in / provisional row replaced).
        let mut changed = all.clone();
        changed[99_990..].fill(3);
        inc.update(99_990, changed.len(), |r| changed[r]);
        full.update(0, changed.len(), |r| changed[r]);
        assert_eq!(inc.buckets, full.buckets);

        // Shorter (another filter): starts again with one row per bucket.
        let few = marks(50, 2);
        inc.update(0, few.len(), |r| few[r]);
        check(&inc, &few);
        assert_eq!(inc.per, 1);
    }

    #[test]
    fn pixel_runs() {
        let mut o = Overview::default();
        let mut m = vec![0u8; 1000];
        m[0] = 2;
        m[1] = 2;
        m[500] = 3;
        m[999] = 1;
        o.update(0, m.len(), |r| m[r]);
        // 100 pixels, 10 rows each.
        assert_eq!(o.runs(100), [(0, 1, 2), (50, 1, 3), (99, 1, 1)]);
        // More pixels than rows: every row gets at least one pixel.
        let mut small = Overview::default();
        let s = [1u8, 1, 0, 2];
        small.update(0, 4, |r| s[r]);
        assert_eq!(small.runs(8), [(0, 4, 1), (6, 2, 2)]);
        assert!(Overview::default().runs(100).is_empty());
    }
}
