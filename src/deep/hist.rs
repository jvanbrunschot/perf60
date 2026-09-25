//! log2 histograms as aggregated by the eBPF programs: bucket `i` counts values in
//! `[2^i, 2^(i+1))` (bucket 0 also holds 0), see `perf60_common::log2_bucket`. Percentiles are
//! reported as the bucket's upper bound, i.e. "at most this", like BCC's histograms.

pub const BUCKETS: usize = 64;

#[derive(Clone, Debug, PartialEq)]
pub struct Hist {
    pub buckets: [u64; BUCKETS],
}

impl Default for Hist {
    fn default() -> Self {
        Hist {
            buckets: [0; BUCKETS],
        }
    }
}

impl Hist {
    /// From per-bucket counts (e.g. summed per-CPU array slots); extra entries are ignored.
    pub fn from_counts(counts: &[u64]) -> Self {
        let mut h = Hist::default();
        for (b, c) in h.buckets.iter_mut().zip(counts) {
            *b = *c;
        }
        h
    }

    pub fn add(&mut self, bucket: usize, n: u64) {
        if let Some(b) = self.buckets.get_mut(bucket) {
            *b += n;
        }
    }

    pub fn count(&self) -> u64 {
        self.buckets.iter().sum()
    }

    /// Upper bound of bucket `i` (exclusive): `2^(i+1)`, saturating.
    pub fn upper(i: usize) -> u64 {
        1u64.checked_shl(i as u32 + 1).unwrap_or(u64::MAX)
    }

    /// Upper bound of the bucket holding the `p`-th percentile (0 < p ≤ 100); `None` if empty.
    pub fn percentile(&self, p: f64) -> Option<u64> {
        let total = self.count();
        if total == 0 {
            return None;
        }
        let rank = ((p / 100.0) * total as f64).ceil().max(1.0) as u64;
        let mut seen = 0;
        for (i, c) in self.buckets.iter().enumerate() {
            seen += c;
            if seen >= rank {
                return Some(Self::upper(i));
            }
        }
        Some(Self::upper(BUCKETS - 1))
    }

    /// Upper bound of the highest non-empty bucket.
    pub fn max(&self) -> Option<u64> {
        self.buckets.iter().rposition(|&c| c > 0).map(Self::upper)
    }

    /// BCC-style distribution lines for the non-empty range, values in microseconds:
    /// `   256 -> 511 µs : 1204 |*******   |`.
    pub fn lines_us(&self, width: usize) -> Vec<String> {
        let Some(first) = self.buckets.iter().position(|&c| c > 0) else {
            return Vec::new();
        };
        let last = self.buckets.iter().rposition(|&c| c > 0).unwrap_or(first);
        let peak = self.buckets[first..=last]
            .iter()
            .copied()
            .max()
            .unwrap_or(1)
            .max(1);
        (first..=last)
            .map(|i| {
                let lo = if i == 0 { 0 } else { 1u64 << i };
                let c = self.buckets[i];
                let stars = ((c as f64 / peak as f64) * width as f64).round() as usize;
                format!(
                    "{:>9} -> {:<9} : {c:>8} |{:<width$}|",
                    us(lo),
                    us(Self::upper(i) - 1),
                    "*".repeat(stars)
                )
            })
            .collect()
    }
}

/// Microseconds, human-readable: `850µs`, `2.1ms`, `1.5s`.
pub fn us(v: u64) -> String {
    match v {
        0..=999 => format!("{v}µs"),
        1_000..=999_999 => trim(v as f64 / 1e3, "ms"),
        _ => trim(v as f64 / 1e6, "s"),
    }
}

fn trim(v: f64, unit: &str) -> String {
    if v >= 100.0 {
        format!("{v:.0}{unit}")
    } else {
        let s = format!("{v:.1}");
        format!("{}{unit}", s.strip_suffix(".0").unwrap_or(&s))
    }
}

/// `p50 12µs, p99 2.1ms, max 8.2ms (45123 events)`; `no events` when empty.
pub fn summary_us(h: &Hist, what: &str) -> String {
    match (h.percentile(50.0), h.percentile(99.0), h.max()) {
        (Some(p50), Some(p99), Some(max)) => format!(
            "p50 {} p99 {} max {} ({} {what})",
            us(p50),
            us(p99),
            us(max),
            h.count()
        ),
        _ => format!("no {what}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn percentiles_use_bucket_upper_bounds() {
        let mut h = Hist::default();
        h.add(3, 90); // 8..15 µs
        h.add(10, 9); // 1024..2047 µs
        h.add(13, 1); // 8192..16383 µs
        assert_eq!(h.count(), 100);
        assert_eq!(h.percentile(50.0), Some(16));
        assert_eq!(h.percentile(90.0), Some(16));
        assert_eq!(h.percentile(99.0), Some(2048));
        assert_eq!(h.percentile(100.0), Some(16384));
        assert_eq!(h.max(), Some(16384));
        assert_eq!(Hist::default().percentile(50.0), None);
        assert_eq!(Hist::upper(63), u64::MAX);
    }

    #[test]
    fn rendering() {
        assert_eq!(us(850), "850µs");
        assert_eq!(us(2048), "2ms");
        assert_eq!(us(2150), "2.1ms");
        assert_eq!(us(1_500_000), "1.5s");
        let h = Hist::from_counts(&[0, 0, 0, 90, 0, 9]);
        let lines = h.lines_us(10);
        assert_eq!(lines.len(), 3);
        assert!(
            lines[0].contains("8µs -> 15µs") && lines[0].ends_with("|**********|"),
            "{}",
            lines[0]
        );
        assert!(lines[1].contains(":        0 |"));
        assert_eq!(
            summary_us(&h, "wakeups"),
            "p50 16µs p99 64µs max 64µs (99 wakeups)"
        );
        assert_eq!(summary_us(&Hist::default(), "wakeups"), "no wakeups");
    }
}
