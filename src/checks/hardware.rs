//! `edac-util` / `cpupower frequency-info` / `chronyc tracking`: hardware and platform state
//! that silently degrades performance and that the 60-second checklist never looks at. ECC
//! memory errors, CPU thermal throttling, CPU frequency and governor, kernel taint and clock
//! discipline.

use std::collections::BTreeMap;

use crate::check::{Check, Context, Resource, Section};
use crate::procfs::platform::{self, TaintFlag};
use crate::source::{ClockStatus, Source};

const EDAC_MC: &str = "/sys/devices/system/edac/mc";
const CPU_DIR: &str = "/sys/devices/system/cpu";
const TAINTED: &str = "/proc/sys/kernel/tainted";

/// A synchronized clock whose max error is above this (µs) gets a note.
const MAXERROR_NOTE_US: i64 = 1_000_000;
/// `powersave` gets a note only on machines with more CPUs than this.
const POWERSAVE_MIN_CPUS: usize = 2;
/// CPUs named in the thermal throttling finding.
const NAMED_CPUS: usize = 8;

const NO_DATA: &str = "no EDAC, thermal throttle, cpufreq, taint or clock data";
const NO_EDAC: &str = "no EDAC memory controllers (VM or driver not loaded)";
const NO_THERMAL: &str = "no thermal throttle counters (VM or not x86)";
const NO_CPUFREQ: &str = "no cpufreq (VM or fixed clock)";

/// One EDAC memory controller.
#[derive(Debug, Clone, PartialEq)]
struct Mc {
    name: String,
    ce: u64,
    ue: u64,
}

/// Thermal throttle counters of one CPU.
#[derive(Debug, Clone, PartialEq)]
struct Throttle {
    cpu: u32,
    core: u64,
    package: Option<u64>,
    /// `topology/physical_package_id`, to count each package's counter once.
    package_id: Option<u64>,
}

/// cpufreq of one CPU. Frequencies are in kHz.
#[derive(Debug, Clone, PartialEq)]
struct Freq {
    cur: Option<u64>,
    max: Option<u64>,
    governor: Option<String>,
}

#[derive(Debug, Clone, Default)]
struct Snapshot {
    edac: Vec<Mc>,
    throttle: Vec<Throttle>,
    freq: Vec<Freq>,
    tainted: Option<u64>,
    clock: Option<ClockStatus>,
}

impl Snapshot {
    fn is_empty(&self) -> bool {
        self.edac.is_empty()
            && self.throttle.is_empty()
            && self.freq.is_empty()
            && self.tainted.is_none()
            && self.clock.is_none()
    }
}

fn counter(src: &dyn Source, path: &str) -> Option<u64> {
    platform::parse_counter(&src.read_to_string(path).ok()?).ok()
}

fn read_edac(src: &dyn Source) -> Vec<Mc> {
    let mut mcs: Vec<(u32, String)> = src
        .read_dir(EDAC_MC)
        .unwrap_or_default()
        .into_iter()
        .filter_map(|n| platform::mc_index(&n).map(|i| (i, n)))
        .collect();
    mcs.sort();
    mcs.into_iter()
        .filter_map(|(_, name)| {
            let ce = counter(src, &format!("{EDAC_MC}/{name}/ce_count"));
            let ue = counter(src, &format!("{EDAC_MC}/{name}/ue_count"));
            (ce.is_some() || ue.is_some()).then(|| Mc {
                ce: ce.unwrap_or(0),
                ue: ue.unwrap_or(0),
                name,
            })
        })
        .collect()
}

fn read_throttle(src: &dyn Source, cpu: u32) -> Option<Throttle> {
    let dir = format!("{CPU_DIR}/cpu{cpu}");
    let core = counter(src, &format!("{dir}/thermal_throttle/core_throttle_count"));
    let package = counter(
        src,
        &format!("{dir}/thermal_throttle/package_throttle_count"),
    );
    if core.is_none() && package.is_none() {
        return None;
    }
    Some(Throttle {
        cpu,
        core: core.unwrap_or(0),
        package,
        package_id: package
            .and_then(|_| counter(src, &format!("{dir}/topology/physical_package_id"))),
    })
}

fn read_freq(src: &dyn Source, cpu: u32) -> Option<Freq> {
    let dir = format!("{CPU_DIR}/cpu{cpu}/cpufreq");
    let f = Freq {
        cur: counter(src, &format!("{dir}/scaling_cur_freq")),
        max: counter(src, &format!("{dir}/cpuinfo_max_freq")).filter(|m| *m > 0),
        governor: src
            .read_to_string(&format!("{dir}/scaling_governor"))
            .ok()
            .and_then(|g| platform::parse_governor(&g).ok()),
    };
    (f.governor.is_some() || (f.cur.is_some() && f.max.is_some())).then_some(f)
}

#[derive(Default)]
pub struct Hardware {
    first: Option<Snapshot>,
    last: Option<Snapshot>,
}

impl Check for Hardware {
    fn id(&self) -> &'static str {
        "hardware"
    }

    fn sample(&mut self, src: &dyn Source, _t: f64) {
        let mut cpus: Vec<u32> = src
            .read_dir(CPU_DIR)
            .unwrap_or_default()
            .iter()
            .filter_map(|n| platform::cpu_index(n))
            .collect();
        cpus.sort_unstable();
        let snap = Snapshot {
            edac: read_edac(src),
            throttle: cpus.iter().filter_map(|c| read_throttle(src, *c)).collect(),
            freq: cpus.iter().filter_map(|c| read_freq(src, *c)).collect(),
            tainted: counter(src, TAINTED),
            clock: src.clock_status().ok(),
        };
        if self.first.is_none() {
            self.first = Some(snap);
        } else {
            self.last = Some(snap);
        }
    }

    fn evaluate(&self, _ctx: &Context) -> Section {
        let s = Section::new(
            "hardware",
            "Hardware and platform",
            "edac-util / cpupower / chronyc tracking",
            Resource::Hardware,
        );
        let Some(first) = &self.first else {
            return s.skipped(NO_DATA);
        };
        let last = self.last.as_ref().unwrap_or(first);
        if last.is_empty() {
            return s.skipped(NO_DATA);
        }
        evaluate(s, first, last)
    }
}

fn evaluate(mut s: Section, first: &Snapshot, last: &Snapshot) -> Section {
    let mut parts = Vec::new();
    edac(&mut s, &mut parts, first, last);
    thermal(&mut s, &mut parts, first, last);
    cpufreq(&mut s, &mut parts, last);
    taint(&mut s, &mut parts, last);
    clock(&mut s, &mut parts, last);
    s.summary(parts.join(", "));
    s
}

fn errors(n: u64) -> &'static str {
    if n == 1 { "error" } else { "errors" }
}

fn edac(s: &mut Section, parts: &mut Vec<String>, first: &Snapshot, last: &Snapshot) {
    if last.edac.is_empty() {
        s.detail(NO_EDAC);
        return;
    }
    let ce: u64 = last.edac.iter().map(|m| m.ce).sum();
    let ue: u64 = last.edac.iter().map(|m| m.ue).sum();
    s.metric("edac_ce", ce as f64);
    s.metric("edac_ue", ue as f64);
    parts.push(match (ue, ce) {
        (0, 0) => "no memory errors".to_owned(),
        (0, c) => format!("{c} corrected memory {}", errors(c)),
        (u, 0) => format!("{u} uncorrectable memory {}", errors(u)),
        (u, c) => format!("{u} uncorrectable and {c} corrected memory {}", errors(c)),
    });

    let mut uncorrectable = Vec::new();
    let mut rising = Vec::new();
    for mc in &last.edac {
        s.detail(format!(
            "{}: {} corrected, {} uncorrectable since boot",
            mc.name, mc.ce, mc.ue
        ));
        if mc.ue > 0 {
            uncorrectable.push(format!("{} on {}", mc.ue, mc.name));
        }
        let before = first.edac.iter().find(|m| m.name == mc.name);
        let d = before.map_or(0, |b| mc.ce.saturating_sub(b.ce));
        if d > 0 {
            rising.push(format!("+{d} on {}", mc.name));
        }
    }
    if !uncorrectable.is_empty() {
        s.crit(format!(
            "uncorrectable memory errors: replace DIMM ({})",
            uncorrectable.join(", ")
        ));
    }
    if !rising.is_empty() {
        s.warn(format!(
            "corrected memory errors during the window: DIMM degrading ({})",
            rising.join(", ")
        ));
    }
    for mc in last.edac.iter().filter(|m| m.ce > 0) {
        s.note(format!(
            "{} corrected memory {} since boot on {}",
            mc.ce,
            errors(mc.ce),
            mc.name
        ));
    }
}

/// Core events summed over CPUs, package events counted once per package.
fn throttle_totals(t: &[Throttle]) -> (u64, u64) {
    let core = t.iter().map(|c| c.core).sum();
    let mut packages: BTreeMap<Option<u64>, u64> = BTreeMap::new();
    for c in t {
        if let Some(p) = c.package {
            let e = packages.entry(c.package_id).or_default();
            *e = (*e).max(p);
        }
    }
    (core, packages.values().sum())
}

fn thermal(s: &mut Section, parts: &mut Vec<String>, first: &Snapshot, last: &Snapshot) {
    if last.throttle.is_empty() {
        s.detail(NO_THERMAL);
        return;
    }
    let (core, package) = throttle_totals(&last.throttle);
    let total = core + package;
    s.metric("thermal_throttle_events", total as f64);
    if total > 0 {
        s.detail(format!(
            "thermal throttling since boot: {core} core, {package} package events"
        ));
        parts.push(format!("{total} thermal throttle events"));
    } else {
        s.detail("no thermal throttling since boot");
        parts.push("no thermal throttling".to_owned());
    }

    let rising: Vec<String> = last
        .throttle
        .iter()
        .filter(|t| {
            first
                .throttle
                .iter()
                .find(|b| b.cpu == t.cpu)
                .is_some_and(|b| t.core > b.core || t.package.unwrap_or(0) > b.package.unwrap_or(0))
        })
        .map(|t| format!("cpu{}", t.cpu))
        .collect();
    if !rising.is_empty() {
        let mut cpus = rising[..rising.len().min(NAMED_CPUS)].join(", ");
        if rising.len() > NAMED_CPUS {
            cpus.push_str(&format!(" and {} more", rising.len() - NAMED_CPUS));
        }
        s.warn(format!(
            "CPU thermal throttling during the window: check cooling and power limits ({cpus})"
        ));
    }
}

fn ghz(khz: f64) -> String {
    format!("{:.1}", khz / 1e6)
}

fn cpufreq(s: &mut Section, parts: &mut Vec<String>, last: &Snapshot) {
    if last.freq.is_empty() {
        s.detail(NO_CPUFREQ);
        return;
    }
    let mut governors: Vec<&str> = Vec::new();
    for g in last.freq.iter().filter_map(|f| f.governor.as_deref()) {
        if !governors.contains(&g) {
            governors.push(g);
        }
    }
    let both: Vec<(u64, u64)> = last.freq.iter().filter_map(|f| f.cur.zip(f.max)).collect();

    let mut detail = Vec::new();
    if !governors.is_empty() {
        detail.push(format!("governor {}", governors.join("/")));
        parts.push(format!("governor {}", governors.join("/")));
    }
    if !both.is_empty() {
        let n = both.len() as f64;
        let cur: u64 = both.iter().map(|(c, _)| c).sum();
        let max: u64 = both.iter().map(|(_, m)| m).sum();
        let pct = cur as f64 * 100.0 / max as f64;
        s.metric("cpufreq_pct_of_max", pct);
        detail.push(format!(
            "{}/{} GHz ({pct:.0}% of max)",
            ghz(cur as f64 / n),
            ghz(max as f64 / n)
        ));
        if governors.is_empty() {
            parts.push(format!("cpufreq {pct:.0}% of max"));
        }
    }
    s.detail(format!("cpufreq: {}", detail.join(", ")));

    if governors.contains(&"powersave") && last.freq.len() > POWERSAVE_MIN_CPUS {
        s.note("powersave governor: expect higher latency; consider performance/schedutil");
    }
}

fn describe(flags: &[&TaintFlag]) -> String {
    flags
        .iter()
        .map(|f| f.describe())
        .collect::<Vec<_>>()
        .join(", ")
}

fn taint(s: &mut Section, parts: &mut Vec<String>, last: &Snapshot) {
    let Some(mask) = last.tainted else {
        return;
    };
    s.metric("tainted", mask as f64);
    if mask == 0 {
        s.detail("kernel not tainted");
        parts.push("kernel not tainted".to_owned());
        return;
    }
    let flags = platform::decode_taint(mask);
    let letters = platform::taint_letters(&flags);
    s.detail(format!(
        "kernel tainted {mask} ({letters}): {}",
        describe(&flags.iter().collect::<Vec<_>>())
    ));
    parts.push(format!("kernel tainted {letters}"));

    let pick = |set: &[char]| -> Vec<&TaintFlag> {
        flags.iter().filter(|f| set.contains(&f.letter)).collect()
    };
    let hardware = pick(&['M', 'B']);
    if !hardware.is_empty() {
        s.crit(format!(
            "machine check / bad page: hardware fault (kernel tainted {})",
            hardware.iter().map(|f| f.letter).collect::<String>()
        ));
    }
    let oops = pick(&['D', 'L']);
    if !oops.is_empty() {
        s.warn(format!(
            "kernel oops / soft lockup since boot: check kernel-log (kernel tainted {})",
            oops.iter().map(|f| f.letter).collect::<String>()
        ));
    }
    let other: Vec<&TaintFlag> = flags
        .iter()
        .filter(|f| !['M', 'B', 'D', 'L'].contains(&f.letter))
        .collect();
    if !other.is_empty() {
        s.note(format!("kernel tainted: {}", describe(&other)));
    }
}

/// Milliseconds with at most one decimal: `45000` µs → `45`, `62770` µs → `62.8`.
fn ms(us: i64) -> String {
    let v = format!("{:.1}", us as f64 / 1000.0);
    v.strip_suffix(".0").unwrap_or(&v).to_owned()
}

fn clock(s: &mut Section, parts: &mut Vec<String>, last: &Snapshot) {
    let Some(c) = last.clock else {
        s.detail("clock status unavailable");
        return;
    };
    let synced = c.synchronized();
    s.metric("clock_synced", if synced { 1.0 } else { 0.0 });
    s.metric("clock_maxerror_ms", c.maxerror_us as f64 / 1000.0);
    s.detail(format!(
        "clock {}, offset {} µs, maxerror {} ms",
        if synced {
            "synchronized"
        } else {
            "not synchronized"
        },
        c.offset_us,
        ms(c.maxerror_us)
    ));
    if synced {
        parts.push("clock synced".to_owned());
        if c.maxerror_us > MAXERROR_NOTE_US {
            s.note(format!(
                "clock max error {} µs is above 1 s: NTP has not corrected the clock recently",
                c.maxerror_us
            ));
        }
    } else {
        parts.push("clock not synced".to_owned());
        s.warn(
            "system clock not synchronized: no NTP/chrony discipline; timestamps and TLS may drift",
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::check::{Level, Status};
    use crate::source::{FsSource, MemSource};
    use crate::sysinfo::SysInfo;

    fn ctx() -> Context {
        Context {
            sys: SysInfo::default(),
            interval: 1.0,
            count: 1,
        }
    }

    fn synced(maxerror_us: i64) -> ClockStatus {
        ClockStatus {
            state: 0,
            status: 0x0001,
            offset_us: 1200,
            maxerror_us,
            esterror_us: 800,
        }
    }

    fn tainted(mask: u64) -> MemSource {
        MemSource::new().with(TAINTED, &format!("{mask}\n"))
    }

    fn set_mc(src: &MemSource, name: &str, ce: u64, ue: u64) {
        src.set(&format!("{EDAC_MC}/{name}/ce_count"), &format!("{ce}\n"));
        src.set(&format!("{EDAC_MC}/{name}/ue_count"), &format!("{ue}\n"));
    }

    fn set_freq(src: &MemSource, cpu: u32, cur: u64, max: u64, gov: &str) {
        let d = format!("{CPU_DIR}/cpu{cpu}/cpufreq");
        src.set(&format!("{d}/scaling_cur_freq"), &format!("{cur}\n"));
        src.set(&format!("{d}/cpuinfo_max_freq"), &format!("{max}\n"));
        src.set(&format!("{d}/scaling_governor"), &format!("{gov}\n"));
    }

    fn set_throttle(src: &MemSource, cpu: u32, core: u64, package: u64) {
        let d = format!("{CPU_DIR}/cpu{cpu}/thermal_throttle");
        src.set(&format!("{d}/core_throttle_count"), &format!("{core}\n"));
        src.set(
            &format!("{d}/package_throttle_count"),
            &format!("{package}\n"),
        );
    }

    fn run(src: &MemSource) -> Section {
        let mut c = Hardware::default();
        c.sample(src, 0.0);
        c.sample(src, 1.0);
        c.evaluate(&ctx())
    }

    /// Sample, apply `change`, sample again.
    fn run_change(src: &MemSource, change: impl Fn(&MemSource)) -> Section {
        let mut c = Hardware::default();
        c.sample(src, 0.0);
        change(src);
        c.sample(src, 1.0);
        c.evaluate(&ctx())
    }

    fn has(s: &Section, level: Level, text: &str) -> bool {
        s.findings
            .iter()
            .any(|f| f.level == level && f.message.contains(text))
    }

    fn detail(s: &Section, line: &str) -> bool {
        s.details.iter().any(|d| d == line)
    }

    #[test]
    fn legacy_summary() {
        let src = tainted(0);
        set_mc(&src, "mc0", 3, 0);
        set_freq(&src, 0, 1_200_000, 3_300_000, "powersave");
        set_freq(&src, 1, 1_200_000, 3_300_000, "powersave");
        src.set(&format!("{CPU_DIR}/online"), "0-1\n");
        src.set_clock(synced(45_000));
        let s = run(&src);
        assert_eq!(s.status, Status::Ok, "{:?}", s.findings);
        assert_eq!(s.resource, Resource::Hardware);
        assert_eq!(
            s.summary,
            "3 corrected memory errors, governor powersave, kernel not tainted, clock synced"
        );
        for line in [
            "mc0: 3 corrected, 0 uncorrectable since boot",
            NO_THERMAL,
            "cpufreq: governor powersave, 1.2/3.3 GHz (36% of max)",
            "kernel not tainted",
            "clock synchronized, offset 1200 µs, maxerror 45 ms",
        ] {
            assert!(detail(&s, line), "{line}: {:?}", s.details);
        }
        assert_eq!(s.metrics["edac_ce"], 3.0);
        assert_eq!(s.metrics["edac_ue"], 0.0);
        assert_eq!(s.metrics["cpufreq_pct_of_max"].round(), 36.0);
        assert_eq!(s.metrics["tainted"], 0.0);
        assert_eq!(s.metrics["clock_synced"], 1.0);
        assert_eq!(s.metrics["clock_maxerror_ms"], 45.0);
        assert!(!s.metrics.contains_key("thermal_throttle_events"));
    }

    #[test]
    fn container_summary() {
        let src = tainted(0);
        src.set_clock(synced(62_770));
        let s = run(&src);
        assert_eq!(s.status, Status::Ok);
        assert_eq!(s.summary, "kernel not tainted, clock synced");
        assert!(detail(&s, NO_EDAC) && detail(&s, NO_THERMAL) && detail(&s, NO_CPUFREQ));
        assert!(detail(
            &s,
            "clock synchronized, offset 1200 µs, maxerror 62.8 ms"
        ));
    }

    #[test]
    fn edac_ue_boundary() {
        let src = tainted(0);
        set_mc(&src, "mc0", 0, 0);
        let s = run(&src);
        assert_eq!(s.status, Status::Ok);
        assert_eq!(s.metrics["edac_ue"], 0.0);
        assert!(s.summary.starts_with("no memory errors"), "{}", s.summary);
        set_mc(&src, "mc0", 0, 1);
        let s = run(&src);
        assert_eq!(s.status, Status::Crit);
        assert!(
            has(&s, Level::Crit, "replace DIMM (1 on mc0)"),
            "{:?}",
            s.findings
        );
        assert!(
            s.summary.starts_with("1 uncorrectable memory error,"),
            "{}",
            s.summary
        );
        set_mc(&src, "mc1", 2, 0);
        assert!(
            run(&src)
                .summary
                .starts_with("1 uncorrectable and 2 corrected memory errors")
        );
    }

    #[test]
    fn edac_ce_rise_warns() {
        let src = tainted(0);
        set_mc(&src, "mc0", 3, 0);
        let s = run_change(&src, |src| set_mc(src, "mc0", 4, 0));
        assert_eq!(s.status, Status::Warn);
        assert!(has(&s, Level::Warn, "+1 on mc0"), "{:?}", s.findings);
        assert!(has(&s, Level::Warn, "DIMM degrading"));
        // A counter reset is not a rise.
        let s = run_change(&src, |src| set_mc(src, "mc0", 0, 0));
        assert_eq!(s.status, Status::Ok);
    }

    #[test]
    fn edac_ce_since_boot_note() {
        let src = tainted(0);
        set_mc(&src, "mc0", 3, 0);
        let s = run(&src);
        assert_eq!(s.status, Status::Ok);
        assert!(
            has(
                &s,
                Level::Note,
                "3 corrected memory errors since boot on mc0"
            ),
            "{:?}",
            s.findings
        );
        set_mc(&src, "mc0", 1, 0);
        assert!(has(
            &run(&src),
            Level::Note,
            "1 corrected memory error since boot on mc0"
        ));
    }

    #[test]
    fn no_edac_controllers() {
        let src = tainted(0).with(&format!("{EDAC_MC}/power/control"), "auto\n");
        let s = run(&src);
        assert_ne!(s.status, Status::Skipped);
        assert!(detail(&s, NO_EDAC), "{:?}", s.details);
        assert!(!s.metrics.contains_key("edac_ce"));
    }

    #[test]
    fn thermal_rise_warns() {
        let src = tainted(0);
        set_throttle(&src, 0, 10, 4);
        set_throttle(&src, 1, 10, 4);
        let s = run_change(&src, |src| set_throttle(src, 1, 12, 4));
        assert_eq!(s.status, Status::Warn);
        assert!(has(&s, Level::Warn, "CPU thermal throttling"));
        assert!(has(&s, Level::Warn, "(cpu1)"), "{:?}", s.findings);
        // Package events alone count too.
        let s = run_change(&src, |src| set_throttle(src, 0, 10, 5));
        assert!(has(&s, Level::Warn, "cpu0"), "{:?}", s.findings);
        // Many CPUs are truncated.
        for cpu in 0..10 {
            set_throttle(&src, cpu, 0, 0);
        }
        let s = run_change(&src, |src| {
            for cpu in 0..10 {
                set_throttle(src, cpu, 1, 0);
            }
        });
        assert!(has(&s, Level::Warn, "cpu7 and 2 more"), "{:?}", s.findings);
    }

    #[test]
    fn thermal_total_detail() {
        let src = tainted(0);
        set_throttle(&src, 0, 10, 4);
        let s = run(&src);
        assert_eq!(s.status, Status::Ok);
        assert_eq!(s.metrics["thermal_throttle_events"], 14.0);
        assert!(detail(
            &s,
            "thermal throttling since boot: 10 core, 4 package events"
        ));
        assert!(
            s.summary.starts_with("14 thermal throttle events"),
            "{}",
            s.summary
        );
        // Two CPUs of one package repeat its counter; two packages add up.
        set_throttle(&src, 1, 0, 4);
        assert_eq!(run(&src).metrics["thermal_throttle_events"], 14.0);
        src.set(
            &format!("{CPU_DIR}/cpu0/topology/physical_package_id"),
            "0\n",
        );
        src.set(
            &format!("{CPU_DIR}/cpu1/topology/physical_package_id"),
            "1\n",
        );
        assert_eq!(run(&src).metrics["thermal_throttle_events"], 18.0);
        let src = tainted(0);
        set_throttle(&src, 0, 0, 0);
        let s = run(&src);
        assert!(detail(&s, "no thermal throttling since boot"));
        assert!(
            s.summary.starts_with("no thermal throttling,"),
            "{}",
            s.summary
        );
    }

    #[test]
    fn no_thermal_counters() {
        let s = run(&tainted(0));
        assert!(detail(&s, NO_THERMAL));
        assert!(!s.metrics.contains_key("thermal_throttle_events"));
    }

    #[test]
    fn powersave_cpu_boundary() {
        let src = tainted(0);
        for cpu in 0..2 {
            set_freq(&src, cpu, 1_000_000, 2_000_000, "powersave");
        }
        let s = run(&src);
        assert!(s.findings.is_empty(), "{:?}", s.findings);
        for cpu in 2..4 {
            set_freq(&src, cpu, 1_000_000, 2_000_000, "powersave");
        }
        let s = run(&src);
        assert_eq!(s.status, Status::Ok);
        assert!(
            has(&s, Level::Note, "powersave governor: expect higher latency"),
            "{:?}",
            s.findings
        );
        assert_eq!(s.metrics["cpufreq_pct_of_max"], 50.0);
        assert!(detail(
            &s,
            "cpufreq: governor powersave, 1.0/2.0 GHz (50% of max)"
        ));
    }

    #[test]
    fn mixed_governors() {
        let src = tainted(0);
        set_freq(&src, 0, 3_000_000, 3_000_000, "performance");
        set_freq(&src, 1, 1_000_000, 3_000_000, "schedutil");
        set_freq(&src, 2, 2_000_000, 3_000_000, "performance");
        let s = run(&src);
        assert!(s.findings.is_empty(), "{:?}", s.findings);
        assert!(
            s.summary.starts_with("governor performance/schedutil,"),
            "{}",
            s.summary
        );
        assert!(detail(
            &s,
            "cpufreq: governor performance/schedutil, 2.0/3.0 GHz (67% of max)"
        ));
        // Frequencies without a governor.
        let src = tainted(0)
            .with(
                &format!("{CPU_DIR}/cpu0/cpufreq/scaling_cur_freq"),
                "900000\n",
            )
            .with(
                &format!("{CPU_DIR}/cpu0/cpufreq/cpuinfo_max_freq"),
                "1800000\n",
            );
        let s = run(&src);
        assert!(
            s.summary.starts_with("cpufreq 50% of max,"),
            "{}",
            s.summary
        );
    }

    #[test]
    fn no_cpufreq() {
        let src = tainted(0).with(&format!("{CPU_DIR}/cpu0/topology/core_id"), "0\n");
        let s = run(&src);
        assert!(detail(&s, NO_CPUFREQ), "{:?}", s.details);
        assert!(!s.metrics.contains_key("cpufreq_pct_of_max"));
    }

    #[test]
    fn taint_levels() {
        let s = run(&tainted(0));
        assert!(detail(&s, "kernel not tainted"));
        assert!(s.findings.is_empty());
        assert_eq!(s.metrics["tainted"], 0.0);

        let s = run(&tainted(1));
        assert_eq!(s.status, Status::Ok);
        assert!(has(&s, Level::Note, "kernel tainted: P proprietary module"));
        assert_eq!(s.summary, "kernel tainted P");
        assert!(detail(&s, "kernel tainted 1 (P): P proprietary module"));

        let s = run(&tainted(128));
        assert_eq!(s.status, Status::Warn);
        assert!(has(&s, Level::Warn, "check kernel-log (kernel tainted D)"));
        assert!(!s.findings.iter().any(|f| f.level == Level::Note));
        assert_eq!(run(&tainted(1 << 14)).status, Status::Warn);

        let s = run(&tainted(16));
        assert_eq!(s.status, Status::Crit);
        assert!(has(&s, Level::Crit, "hardware fault (kernel tainted M)"));
        assert_eq!(run(&tainted(32)).status, Status::Crit);

        let s = run(&tainted(4608));
        assert_eq!(s.status, Status::Ok);
        assert!(s.summary.contains("kernel tainted WO"), "{}", s.summary);
        assert!(
            has(
                &s,
                Level::Note,
                "kernel tainted: W kernel warning, O out-of-tree module"
            ),
            "{:?}",
            s.findings
        );
        assert_eq!(s.metrics["tainted"], 4608.0);

        // M + D + W: one finding per class.
        let s = run(&tainted(16 | 128 | 512));
        assert_eq!(s.status, Status::Crit);
        assert_eq!(s.findings.len(), 3, "{:?}", s.findings);
    }

    #[test]
    fn clock_unsynced_warns() {
        let src = tainted(0);
        src.set_clock(ClockStatus {
            state: ClockStatus::TIME_ERROR,
            status: ClockStatus::STA_UNSYNC,
            offset_us: 0,
            maxerror_us: 16_000_000,
            esterror_us: 16_000_000,
        });
        let s = run(&src);
        assert_eq!(s.status, Status::Warn);
        assert_eq!(s.findings.len(), 1, "{:?}", s.findings);
        assert!(has(&s, Level::Warn, "system clock not synchronized"));
        assert_eq!(s.metrics["clock_synced"], 0.0);
        assert!(s.summary.contains("clock not synced"), "{}", s.summary);
        assert!(detail(
            &s,
            "clock not synchronized, offset 0 µs, maxerror 16000 ms"
        ));
    }

    #[test]
    fn clock_maxerror_boundary() {
        let src = tainted(0);
        src.set_clock(synced(1_000_000));
        let s = run(&src);
        assert!(s.findings.is_empty(), "{:?}", s.findings);
        assert_eq!(s.metrics["clock_maxerror_ms"], 1000.0);
        src.set_clock(synced(1_000_001));
        let s = run(&src);
        assert_eq!(s.status, Status::Ok);
        assert!(
            has(&s, Level::Note, "clock max error 1000001 µs is above 1 s"),
            "{:?}",
            s.findings
        );
    }

    #[test]
    fn clock_unavailable() {
        let s = run(&tainted(0));
        assert!(detail(&s, "clock status unavailable"));
        assert!(!s.metrics.contains_key("clock_synced"));
        assert_eq!(s.summary, "kernel not tainted");
    }

    #[test]
    fn all_sources_absent_skipped() {
        let src = MemSource::new()
            .with(&format!("{CPU_DIR}/online"), "0-3\n")
            .with(&format!("{EDAC_MC}/power/control"), "auto\n")
            .with(TAINTED, "garbage\n");
        let s = run(&src);
        assert_eq!(s.status, Status::Skipped);
        assert_eq!(s.summary, NO_DATA);
        let s = Hardware::default().evaluate(&ctx());
        assert_eq!(s.status, Status::Skipped);
        // Any one source is enough.
        let src = MemSource::new();
        src.set_clock(synced(0));
        assert_ne!(run(&src).status, Status::Skipped);
    }

    #[test]
    fn fixture_trees() {
        let tree = |name: &str| {
            let src = FsSource::new(format!(
                "{}/tests/fixtures/{name}",
                env!("CARGO_MANIFEST_DIR")
            ));
            let mut c = Hardware::default();
            c.sample(&src, 0.0);
            c.sample(&src, 0.0);
            c.evaluate(&ctx())
        };
        let arm = tree("linux-arm64");
        assert_eq!(arm.status, Status::Ok, "{:?}", arm.findings);
        assert_eq!(arm.summary, "kernel not tainted, clock synced");
        assert!(detail(&arm, NO_EDAC) && detail(&arm, NO_CPUFREQ));
        let legacy = tree("linux-legacy");
        assert_eq!(legacy.status, Status::Ok, "{:?}", legacy.findings);
        assert_eq!(
            legacy.summary,
            "3 corrected memory errors, governor powersave, kernel not tainted, clock synced"
        );
        assert!(detail(
            &legacy,
            "cpufreq: governor powersave, 1.2/3.3 GHz (36% of max)"
        ));
        assert!(detail(
            &legacy,
            "clock synchronized, offset 1200 µs, maxerror 45 ms"
        ));
    }
}
