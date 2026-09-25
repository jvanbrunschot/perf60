//! Hardware and platform files: single-number sysfs counters (EDAC `ce_count`/`ue_count`,
//! `thermal_throttle/*_throttle_count`, cpufreq kHz values), `scaling_governor`, the `cpuN` /
//! `mcN` directory names, and the `/proc/sys/kernel/tainted` bitmask.

use super::{Result, err};

/// A single unsigned number, e.g. `3\n` from `ce_count` or `3300000\n` from `cpuinfo_max_freq`.
pub fn parse_counter(input: &str) -> Result<u64> {
    let t = input.trim();
    t.parse()
        .map_err(|_| super::ParseError(format!("expected a number, got {t:?}")))
}

/// `scaling_governor`: one word such as `powersave` or `schedutil`.
pub fn parse_governor(input: &str) -> Result<String> {
    match input.split_whitespace().collect::<Vec<_>>()[..] {
        [g] => Ok(g.to_owned()),
        _ => err(format!(
            "expected one governor name, got {:?}",
            input.trim()
        )),
    }
}

fn index(name: &str, prefix: &str) -> Option<u32> {
    let n = name.strip_prefix(prefix)?;
    if n.is_empty() || !n.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    n.parse().ok()
}

/// `cpu12` → 12. Other entries of `/sys/devices/system/cpu` (`cpufreq`, `cpuidle`, `online`)
/// are `None`.
pub fn cpu_index(name: &str) -> Option<u32> {
    index(name, "cpu")
}

/// `mc0` → 0, for the entries of `/sys/devices/system/edac/mc`.
pub fn mc_index(name: &str) -> Option<u32> {
    index(name, "mc")
}

/// The taint flags of `Documentation/admin-guide/tainted-kernels.rst`, by bit number.
pub const TAINT_FLAGS: [(char, &str); 19] = [
    ('P', "proprietary module"),
    ('F', "module force-loaded"),
    ('S', "unsafe SMP"),
    ('R', "module force-unloaded"),
    ('M', "machine check"),
    ('B', "bad page"),
    ('U', "user taint"),
    ('D', "kernel died (oops)"),
    ('A', "ACPI table overridden"),
    ('W', "kernel warning"),
    ('C', "staging driver"),
    ('I', "firmware workaround"),
    ('O', "out-of-tree module"),
    ('E', "unsigned module"),
    ('L', "soft lockup"),
    ('K', "live patched"),
    ('X', "auxiliary taint"),
    ('T', "randomized struct layout"),
    ('N', "test module"),
];

/// One set taint bit. Bits this table does not know have letter `?`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TaintFlag {
    pub bit: u32,
    pub letter: char,
    pub meaning: &'static str,
}

impl TaintFlag {
    /// `W kernel warning`, or `? unknown (bit 20)`.
    pub fn describe(&self) -> String {
        if self.letter == '?' {
            format!("? unknown (bit {})", self.bit)
        } else {
            format!("{} {}", self.letter, self.meaning)
        }
    }
}

/// The set bits of a taint mask, lowest bit first.
pub fn decode_taint(mask: u64) -> Vec<TaintFlag> {
    (0..64)
        .filter(|b| mask & (1u64 << b) != 0)
        .map(|bit| {
            let (letter, meaning) = TAINT_FLAGS
                .get(bit as usize)
                .copied()
                .unwrap_or(('?', "unknown"));
            TaintFlag {
                bit,
                letter,
                meaning,
            }
        })
        .collect()
}

/// The letters of the set flags, e.g. `WO` for 4608.
pub fn taint_letters(flags: &[TaintFlag]) -> String {
    flags.iter().map(|f| f.letter).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_counters() {
        assert_eq!(parse_counter("3\n"), Ok(3));
        assert_eq!(parse_counter(" 3300000 "), Ok(3300000));
        assert!(parse_counter("").is_err());
        assert!(parse_counter("-1\n").is_err());
        assert!(parse_counter("<unsupported>\n").is_err());
    }

    #[test]
    fn parses_governor() {
        assert_eq!(parse_governor("powersave\n").unwrap(), "powersave");
        assert!(parse_governor("\n").is_err());
        assert!(parse_governor("a b\n").is_err());
    }

    #[test]
    fn directory_indexes() {
        assert_eq!(cpu_index("cpu0"), Some(0));
        assert_eq!(cpu_index("cpu127"), Some(127));
        for n in ["cpufreq", "cpuidle", "cpu", "online", "cpu1a"] {
            assert_eq!(cpu_index(n), None, "{n}");
        }
        assert_eq!(mc_index("mc3"), Some(3));
        assert_eq!(mc_index("power"), None);
        assert_eq!(mc_index("mc"), None);
    }

    #[test]
    fn decodes_taint_letters() {
        assert!(decode_taint(0).is_empty());
        let p = decode_taint(1);
        assert_eq!(taint_letters(&p), "P");
        assert_eq!(p[0].describe(), "P proprietary module");
        assert_eq!(taint_letters(&decode_taint(128)), "D");
        assert_eq!(taint_letters(&decode_taint(16)), "M");
        let wo = decode_taint(4608);
        assert_eq!(taint_letters(&wo), "WO");
        assert_eq!(wo[0].bit, 9);
        assert_eq!(wo[1].describe(), "O out-of-tree module");
        let all = decode_taint((1 << 19) - 1);
        assert_eq!(taint_letters(&all), "PFSRMBUDAWCIOELKXTN");
    }

    #[test]
    fn unknown_taint_bits() {
        let f = decode_taint((1 << 20) | (1 << 9));
        assert_eq!(taint_letters(&f), "W?");
        assert_eq!(f[1].describe(), "? unknown (bit 20)");
    }

    #[test]
    fn parses_fixture_files() {
        let tainted = include_str!("../../tests/fixtures/linux-arm64/proc/sys/kernel/tainted");
        assert_eq!(parse_counter(tainted), Ok(0));
        let ce = include_str!(
            "../../tests/fixtures/linux-legacy/sys/devices/system/edac/mc/mc0/ce_count"
        );
        assert_eq!(parse_counter(ce), Ok(3));
        let gov = include_str!(
            "../../tests/fixtures/linux-legacy/sys/devices/system/cpu/cpu0/cpufreq/scaling_governor"
        );
        assert_eq!(parse_governor(gov).unwrap(), "powersave");
        let max = include_str!(
            "../../tests/fixtures/linux-legacy/sys/devices/system/cpu/cpu0/cpufreq/cpuinfo_max_freq"
        );
        assert_eq!(parse_counter(max), Ok(3300000));
    }
}
