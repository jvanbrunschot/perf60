//! Human-readable formatting shared by checks and reports.

/// Binary-prefixed size: `1536` → `1.5 KiB`.
pub fn bytes(n: u64) -> String {
    const UNITS: [&str; 6] = ["B", "KiB", "MiB", "GiB", "TiB", "PiB"];
    let mut v = n as f64;
    let mut i = 0;
    while v >= 1024.0 && i < UNITS.len() - 1 {
        v /= 1024.0;
        i += 1;
    }
    if i == 0 {
        format!("{n} B")
    } else if v >= 100.0 {
        format!("{v:.0} {}", UNITS[i])
    } else {
        let v = format!("{v:.1}");
        format!("{} {}", v.strip_suffix(".0").unwrap_or(&v), UNITS[i])
    }
}

/// Byte rate: `bytes(n)/s`.
pub fn bytes_rate(per_sec: f64) -> String {
    format!("{}/s", bytes(per_sec.max(0.0) as u64))
}

/// Compact duration: `93784` → `1d 2h03m`.
pub fn duration(secs: f64) -> String {
    let s = secs.max(0.0) as u64;
    let (d, h, m) = (s / 86400, (s % 86400) / 3600, (s % 3600) / 60);
    if d > 0 {
        format!("{d}d {h}h{m:02}m")
    } else if h > 0 {
        format!("{h}h{m:02}m")
    } else {
        format!("{m}m{:02}s", s % 60)
    }
}

/// CPU count that may be fractional (cgroup quota): `4.0` → `4`, `1.5` → `1.5`.
pub fn cpus(n: f64) -> String {
    if (n - n.round()).abs() < 1e-9 {
        format!("{}", n.round() as u64)
    } else {
        format!("{n:.2}").trim_end_matches('0').to_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats() {
        assert_eq!(bytes(512), "512 B");
        assert_eq!(bytes(1536), "1.5 KiB");
        assert_eq!(bytes(2034011136), "1.9 GiB");
        assert_eq!(bytes(200 * 1024 * 1024), "200 MiB");
        assert_eq!(bytes_rate(2048.0), "2 KiB/s");
        assert_eq!(bytes(16 << 30), "16 GiB");
        assert_eq!(duration(93784.0), "1d 2h03m");
        assert_eq!(duration(14937.0), "4h08m");
        assert_eq!(duration(75.0), "1m15s");
        assert_eq!(cpus(4.0), "4");
        assert_eq!(cpus(1.5), "1.5");
        assert_eq!(cpus(0.25), "0.25");
    }
}
