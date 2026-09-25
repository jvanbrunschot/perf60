//! Capacity and limits the 60-second checklist never looks at, yet which cause outages:
//! `df -h` / `df -i` (filesystems) and `ulimit` / `file-nr` / `pid_max` (limits).

use crate::check::{Check, Context, Resource, SampleError, Section};
use crate::procfs::limits::{self, FileNr};
use crate::procfs::mounts::{self, Mount};
use crate::procfs::{loadavg, pid_stat, system};
use crate::source::{FsStat, Source};
use crate::units;

const MOUNTS: &str = "/proc/self/mounts";
const FILE_NR: &str = "/proc/sys/fs/file-nr";
const LOADAVG: &str = "/proc/loadavg";
const PID_MAX: &str = "/proc/sys/kernel/pid_max";
const THREADS_MAX: &str = "/proc/sys/kernel/threads-max";
const SELF_CGROUP: &str = "/proc/self/cgroup";
const PROC: &str = "/proc";

/// Filesystem types without meaningful capacity. squashfs is always 100% full by design.
const PSEUDO_FS: &[&str] = &[
    "proc",
    "sysfs",
    "devtmpfs",
    "devpts",
    "cgroup",
    "cgroup2",
    "mqueue",
    "debugfs",
    "tracefs",
    "securityfs",
    "pstore",
    "bpf",
    "autofs",
    "configfs",
    "fusectl",
    "hugetlbfs",
    "binfmt_misc",
    "nsfs",
    "rpc_pipefs",
    "selinuxfs",
    "efivarfs",
    "ramfs",
    "rootfs",
    "squashfs",
];
/// Types that are normally mounted read-write; `ro` on these often means errors=remount-ro.
const WRITABLE_FS: &[&str] = &["ext2", "ext3", "ext4", "xfs", "btrfs", "f2fs", "vfat"];
const RO_NOTE: &str = "mounted read-only (possibly remounted after errors, check kernel-log)";

/// Space or inode use above these percentages is WARN / CRIT.
const FS_WARN_PCT: f64 = 85.0;
const FS_CRIT_PCT: f64 = 95.0;
/// System-wide limits (file handles, pids, threads, cgroup pids) above these are WARN / CRIT.
const LIMIT_WARN_PCT: f64 = 80.0;
const LIMIT_CRIT_PCT: f64 = 90.0;
/// A process above this percentage of its soft open-files limit is WARN.
const PROC_FD_WARN_PCT: f64 = 90.0;
/// Processes named in the open-files finding.
const NAMED: usize = 3;
/// file-max of LONG_MAX (the default on recent kernels) means no practical limit.
const FILE_MAX_UNLIMITED: u64 = i64::MAX as u64;

/// `a` as a percentage of `b`; 0 when `b` is 0 (never NaN).
fn pct(a: u64, b: u64) -> f64 {
    if b == 0 {
        0.0
    } else {
        a as f64 * 100.0 / b as f64
    }
}

/// df-style display: the percentage rounded up to a whole number.
fn pct_ceil(a: u64, b: u64) -> u64 {
    if b == 0 {
        0
    } else {
        ((a as u128 * 100).div_ceil(b as u128)) as u64
    }
}

/// One decimal, without a trailing `.0`: `0.8`, `93.8`, `80`.
fn pct1(p: f64) -> String {
    let s = format!("{p:.1}");
    s.strip_suffix(".0").unwrap_or(&s).to_owned()
}

// ---------------------------------------------------------------------------------------------
// filesystems (df -h / df -i)

struct Fs {
    mount: Mount,
    st: FsStat,
}

impl Fs {
    fn used_blocks(&self) -> u64 {
        self.st.blocks.saturating_sub(self.st.bfree)
    }

    /// Like df: the root reserve counts as unavailable, so used ÷ (used + available).
    fn used_pct(&self) -> f64 {
        let used = self.used_blocks();
        pct(used, used + self.st.bavail)
    }

    fn used_pct_display(&self) -> u64 {
        let used = self.used_blocks();
        pct_ceil(used, used + self.st.bavail)
    }

    fn avail_bytes(&self) -> u64 {
        self.st.bavail.saturating_mul(self.st.frsize)
    }

    fn size_bytes(&self) -> u64 {
        self.st.blocks.saturating_mul(self.st.frsize)
    }

    fn inodes_used(&self) -> u64 {
        self.st.files.saturating_sub(self.st.ffree)
    }

    /// None when the filesystem has no fixed inode count (btrfs reports 0).
    fn inodes_used_pct(&self) -> Option<f64> {
        (self.st.files > 0).then(|| pct(self.inodes_used(), self.st.files))
    }

    fn read_only(&self) -> bool {
        self.mount.read_only() || self.st.readonly
    }
}

#[derive(Default)]
pub struct Filesystems {
    /// Filesystems of the last sample in which the mount table and a statvfs call succeeded.
    fs: Option<Vec<Fs>>,
    error: SampleError,
}

impl Check for Filesystems {
    fn id(&self) -> &'static str {
        "filesystems"
    }

    fn sample(&mut self, src: &dyn Source, _t: f64) {
        let Some(text) = self.error.read(src, MOUNTS) else {
            return;
        };
        let table = match mounts::parse(&text) {
            Ok(m) => m,
            Err(e) => {
                self.error.record(MOUNTS, &std::io::Error::other(e.0));
                return;
            }
        };
        let mut found = Vec::new();
        let mut failed = SampleError::default();
        for m in candidates(table) {
            match src.statvfs(&m.mount_point) {
                Ok(st) if st.blocks > 0 => found.push(Fs { mount: m, st }),
                Ok(_) => {}
                Err(e) => failed.record(&format!("statvfs {}", m.mount_point), &e),
            }
        }
        match failed.get() {
            Some(reason) if found.is_empty() => {
                self.error
                    .record(MOUNTS, &std::io::Error::other(reason.to_owned()));
            }
            _ => self.fs = Some(dedupe(found)),
        }
    }

    fn evaluate(&self, ctx: &Context) -> Section {
        let s = Section::new(
            "filesystems",
            "Filesystems",
            "df -h / df -i",
            Resource::Capacity,
        );
        match &self.fs {
            None => s.skipped(self.error.get().unwrap_or("no samples")),
            Some(fs) if fs.is_empty() => {
                s.skipped(format!("no filesystem with capacity in {MOUNTS}"))
            }
            Some(fs) => evaluate_filesystems(s, fs, ctx.sys.container),
        }
    }
}

/// Real filesystems: pseudo types dropped, and only the last (visible) mount per mount point.
fn candidates(table: Vec<Mount>) -> Vec<Mount> {
    let mut out: Vec<Mount> = Vec::new();
    for m in table {
        if PSEUDO_FS.contains(&m.fstype.as_str()) {
            continue;
        }
        out.retain(|o| o.mount_point != m.mount_point);
        out.push(m);
    }
    out
}

/// One entry per filesystem: bind mounts of the same (source, type, blocks, files) keep the
/// shortest mount point (lexically first on a tie), at the position of the first occurrence.
fn dedupe(found: Vec<Fs>) -> Vec<Fs> {
    let mut out: Vec<Fs> = Vec::new();
    for f in found {
        let same = |o: &Fs| {
            o.mount.source == f.mount.source
                && o.mount.fstype == f.mount.fstype
                && o.st.blocks == f.st.blocks
                && o.st.files == f.st.files
        };
        match out.iter_mut().find(|o| same(o)) {
            Some(o) => {
                let (a, b) = (&f.mount.mount_point, &o.mount.mount_point);
                if (a.len(), a) < (b.len(), b) {
                    *o = f;
                }
            }
            None => out.push(f),
        }
    }
    out
}

fn evaluate_filesystems(mut s: Section, fs: &[Fs], container: bool) -> Section {
    let lead = fs
        .iter()
        .find(|f| f.mount.mount_point == "/")
        .unwrap_or(&fs[0]);
    let fullest = fs
        .iter()
        .reduce(|a, b| if b.used_pct() > a.used_pct() { b } else { a })
        .unwrap_or(lead);
    let noun = if fs.len() == 1 {
        "filesystem"
    } else {
        "filesystems"
    };
    let mut summary = format!(
        "{} {}% used ({} free), {} {noun}",
        lead.mount.mount_point,
        lead.used_pct_display(),
        units::bytes(lead.avail_bytes()),
        fs.len()
    );
    if fullest.mount.mount_point != lead.mount.mount_point {
        summary.push_str(&format!(
            ", fullest {} {}%",
            fullest.mount.mount_point,
            fullest.used_pct_display()
        ));
    }
    s.summary(summary);
    s.metric("max_used_pct", fullest.used_pct());

    for f in fs {
        let mp = &f.mount.mount_point;
        let inodes = match f.inodes_used_pct() {
            Some(_) => format!("{}%", pct_ceil(f.inodes_used(), f.st.files)),
            None => "-".to_owned(),
        };
        s.detail(format!(
            "{mp} {}: size {}, used {}%, avail {}, inodes {inodes}",
            f.mount.fstype,
            units::bytes(f.size_bytes()),
            f.used_pct_display(),
            units::bytes(f.avail_bytes()),
        ));
        s.metric(format!("{mp}.used_pct"), f.used_pct());
        s.threshold(
            f.used_pct(),
            FS_WARN_PCT,
            FS_CRIT_PCT,
            format!(
                "{mp} is {}% full ({} free): writes fail with ENOSPC when it fills up",
                f.used_pct_display(),
                units::bytes(f.avail_bytes())
            ),
        );
        if let Some(ip) = f.inodes_used_pct() {
            s.metric(format!("{mp}.inodes_used_pct"), ip);
            s.threshold(
                ip,
                FS_WARN_PCT,
                FS_CRIT_PCT,
                format!(
                    "{mp} uses {}% of its inodes ({} free): creating files fails with ENOSPC when they run out",
                    pct_ceil(f.inodes_used(), f.st.files),
                    f.st.ffree
                ),
            );
        }
        let intentional = container && mp == "/";
        if f.read_only() && WRITABLE_FS.contains(&f.mount.fstype.as_str()) && !intentional {
            s.note(format!("{mp} ({}) {RO_NOTE}", f.mount.fstype));
        }
    }
    s
}

// ---------------------------------------------------------------------------------------------
// limits (ulimit / file-nr / pid_max)

/// The cgroup level whose pids limit is closest to being hit.
#[derive(Debug, Clone, PartialEq)]
struct CgroupPids {
    dir: String,
    current: u64,
    /// None: `max` (no limit).
    max: Option<u64>,
}

impl CgroupPids {
    fn used_pct(&self) -> Option<f64> {
        self.max.map(|m| pct(self.current, m))
    }
}

/// Open fds of one process against its soft `RLIMIT_NOFILE`.
#[derive(Debug, Clone, PartialEq)]
struct ProcFds {
    pid: u32,
    comm: String,
    fds: u64,
    limit: u64,
}

impl ProcFds {
    fn used_pct(&self) -> f64 {
        pct(self.fds, self.limit)
    }

    fn label(&self) -> String {
        format!("{}({}) {}/{}", self.comm, self.pid, self.fds, self.limit)
    }
}

pub struct Limits {
    file_nr: Option<FileNr>,
    tasks: Option<u64>,
    pid_max: Option<u64>,
    threads_max: Option<u64>,
    cgroup: Option<CgroupPids>,
    /// Readable processes of the last scan, highest fd use first.
    procs: Option<Vec<ProcFds>>,
    error: SampleError,
    own_pid: u32,
}

impl Default for Limits {
    fn default() -> Self {
        Limits {
            file_nr: None,
            tasks: None,
            pid_max: None,
            threads_max: None,
            cgroup: None,
            procs: None,
            error: SampleError::default(),
            own_pid: std::process::id(),
        }
    }
}

fn read_number(err: &mut SampleError, src: &dyn Source, path: &str) -> Option<u64> {
    let text = err.read(src, path)?;
    match limits::parse_number(&text) {
        Ok(n) => Some(n),
        Err(e) => {
            err.record(path, &std::io::Error::other(e.0));
            None
        }
    }
}

impl Check for Limits {
    fn id(&self) -> &'static str {
        "limits"
    }

    fn sample(&mut self, src: &dyn Source, _t: f64) {
        // file-nr first, so a SKIPPED reason names it.
        if let Some(text) = self.error.read(src, FILE_NR) {
            match limits::parse_file_nr(&text) {
                Ok(n) => self.file_nr = Some(n),
                Err(e) => self.error.record(FILE_NR, &std::io::Error::other(e.0)),
            }
        }
        if let Some(total) = self
            .error
            .read(src, LOADAVG)
            .and_then(|s| loadavg::parse(&s).ok())
            .map(|l| l.total)
            .filter(|t| *t > 0)
        {
            self.tasks = Some(total);
        }
        if let Some(n) = read_number(&mut self.error, src, PID_MAX) {
            self.pid_max = Some(n);
        }
        if let Some(n) = read_number(&mut self.error, src, THREADS_MAX) {
            self.threads_max = Some(n);
        }
        if let Some(cg) = cgroup_pids(src) {
            self.cgroup = Some(cg);
        }
        if let Some(p) = scan_fds(src, self.own_pid) {
            self.procs = Some(p);
        }
    }

    fn evaluate(&self, _ctx: &Context) -> Section {
        let s = Section::new(
            "limits",
            "Kernel and process limits",
            "ulimit / file-nr / pid_max",
            Resource::Capacity,
        );
        let tasks = self
            .tasks
            .filter(|_| self.pid_max.is_some() || self.threads_max.is_some());
        if self.file_nr.is_none()
            && tasks.is_none()
            && self.cgroup.is_none()
            && self.procs.is_none()
        {
            return s.skipped(self.error.get().unwrap_or("no samples"));
        }
        evaluate_limits(
            s,
            self.file_nr,
            tasks.map(|t| (t, self.pid_max, self.threads_max)),
            self.cgroup.as_ref(),
            self.procs.as_deref(),
        )
    }
}

/// The own cgroup's pids use. v1: the `pids` controller path below `/sys/fs/cgroup/pids`; v2:
/// the `0::` path below `/sys/fs/cgroup`. Walks up to the mount root (inside a container the
/// listed path usually doesn't exist below it) and keeps the level with the highest use; a level
/// without a limit is only kept when no level has one.
fn cgroup_pids(src: &dyn Source) -> Option<CgroupPids> {
    let own = src.read_to_string(SELF_CGROUP).ok();
    let (base, path) = match own.as_deref().and_then(|s| limits::cgroup1_path(s, "pids")) {
        Some(p) => ("/sys/fs/cgroup/pids", p),
        None => (
            "/sys/fs/cgroup",
            own.as_deref()
                .and_then(system::cgroup2_path)
                .unwrap_or_else(|| "/".to_owned()),
        ),
    };
    let mut path = path.trim_end_matches('/').to_owned();
    let mut best: Option<CgroupPids> = None;
    loop {
        let dir = format!("{base}{path}");
        let read = |f: &str| src.read_to_string(&format!("{dir}/{f}")).ok();
        if let (Some(Ok(current)), Some(Ok(max))) = (
            read("pids.current").map(|s| limits::parse_number(&s)),
            read("pids.max").map(|s| limits::parse_pids_max(&s)),
        ) {
            let cg = CgroupPids { dir, current, max };
            let better = match (&best, cg.used_pct()) {
                (None, _) => true,
                (Some(b), Some(p)) => b.used_pct().is_none_or(|bp| p > bp),
                (Some(_), None) => false,
            };
            if better {
                best = Some(cg);
            }
        }
        match path.rfind('/') {
            Some(i) => path.truncate(i),
            None => break,
        }
    }
    best
}

/// Open fds of every process whose fd directory and limits are readable. Other users'
/// processes (without root) and processes that exit mid-scan are skipped silently, as are
/// unlimited soft limits.
fn scan_fds(src: &dyn Source, own_pid: u32) -> Option<Vec<ProcFds>> {
    let mut out = Vec::new();
    for name in src.read_dir(PROC).ok()? {
        let Ok(pid) = name.parse::<u32>() else {
            continue;
        };
        if pid == own_pid {
            continue;
        }
        let Ok(fds) = src.read_dir(&format!("{PROC}/{pid}/fd")) else {
            continue;
        };
        let Some(limit) = src
            .read_to_string(&format!("{PROC}/{pid}/limits"))
            .ok()
            .and_then(|s| limits::max_open_files(&s).ok())
            .flatten()
            .filter(|l| *l > 0)
        else {
            continue;
        };
        let comm = src
            .read_to_string(&format!("{PROC}/{pid}/stat"))
            .ok()
            .and_then(|s| pid_stat::parse(&s).ok())
            .map_or_else(|| "?".to_owned(), |st| st.comm);
        out.push(ProcFds {
            pid,
            comm,
            fds: fds.len() as u64,
            limit,
        });
    }
    if out.is_empty() {
        return None;
    }
    out.sort_by(|a, b| {
        b.used_pct()
            .total_cmp(&a.used_pct())
            .then(b.fds.cmp(&a.fds))
            .then(a.pid.cmp(&b.pid))
    });
    Some(out)
}

fn evaluate_limits(
    mut s: Section,
    file_nr: Option<FileNr>,
    tasks: Option<(u64, Option<u64>, Option<u64>)>,
    cgroup: Option<&CgroupPids>,
    procs: Option<&[ProcFds]>,
) -> Section {
    let mut summary: Vec<String> = Vec::new();

    if let Some(n) = file_nr {
        let p = pct(n.allocated, n.max);
        if n.max >= FILE_MAX_UNLIMITED {
            summary.push(format!("fds {}/unlimited", n.allocated));
            s.detail(format!(
                "file handles: {} allocated, {} unused, max unlimited",
                n.allocated, n.unused
            ));
        } else {
            summary.push(format!("fds {}/{} ({}%)", n.allocated, n.max, pct1(p)));
            s.detail(format!(
                "file handles: {} allocated, {} unused, max {} ({}%)",
                n.allocated,
                n.unused,
                n.max,
                pct1(p)
            ));
        }
        s.metric("file_nr_pct", p);
        s.threshold(
            p,
            LIMIT_WARN_PCT,
            LIMIT_CRIT_PCT,
            format!(
                "{} of {} system-wide file handles in use ({}%): open() fails with ENFILE at fs.file-max",
                n.allocated,
                n.max,
                pct1(p)
            ),
        );
    }

    if let Some((t, pid_max, threads_max)) = tasks {
        let mut detail = format!("tasks: {t}");
        if let Some(limit) = pid_max.or(threads_max) {
            summary.push(format!("tasks {t}/{limit}"));
        }
        for (name, key, limit) in [
            ("pid_max", "tasks_pct", pid_max),
            ("threads-max", "threads_pct", threads_max),
        ] {
            let Some(limit) = limit else {
                continue;
            };
            let p = pct(t, limit);
            detail.push_str(&format!(", {name} {limit} ({}%)", pct1(p)));
            s.metric(key, p);
            s.threshold(
                p,
                LIMIT_WARN_PCT,
                LIMIT_CRIT_PCT,
                format!(
                    "{t} tasks (threads) use {}% of kernel.{name} {limit}: fork() and clone() fail with EAGAIN at the limit",
                    pct1(p)
                ),
            );
        }
        s.detail(detail);
    }

    if let Some(cg) = cgroup {
        match (cg.max, cg.used_pct()) {
            (Some(max), Some(p)) => {
                summary.push(format!("cgroup pids {}/{max}", cg.current));
                s.detail(format!(
                    "cgroup pids: {}/{max} ({}%) in {}",
                    cg.current,
                    pct1(p),
                    cg.dir
                ));
                s.metric("cgroup_pids_pct", p);
                s.threshold(
                    p,
                    LIMIT_WARN_PCT,
                    LIMIT_CRIT_PCT,
                    format!(
                        "cgroup pids {}/{max} ({}%) in {}: fork() in this cgroup fails with EAGAIN at the limit",
                        cg.current,
                        pct1(p),
                        cg.dir
                    ),
                );
            }
            _ => s.detail(format!(
                "cgroup pids: {} in {} (no limit)",
                cg.current, cg.dir
            )),
        }
    }

    if let Some(procs) = procs
        && let Some(top) = procs.first()
    {
        summary.push(format!("highest process {} fds", top.label()));
        let listed: Vec<String> = procs
            .iter()
            .take(NAMED)
            .map(|p| format!("{} ({}%)", p.label(), pct1(p.used_pct())))
            .collect();
        let readable = if procs.len() == 1 {
            "1 process readable".to_owned()
        } else {
            format!("{} processes readable", procs.len())
        };
        s.detail(format!(
            "open files: {readable}, highest {}",
            listed.join(", ")
        ));
        s.metric("max_process_fd_pct", top.used_pct());
        let over: Vec<&ProcFds> = procs
            .iter()
            .filter(|p| p.used_pct() > PROC_FD_WARN_PCT)
            .collect();
        if !over.is_empty() {
            let mut named: Vec<String> = over.iter().take(NAMED).map(|p| p.label()).collect();
            if over.len() > NAMED {
                named.push(format!("+{} more", over.len() - NAMED));
            }
            let noun = if over.len() == 1 {
                "process is"
            } else {
                "processes are"
            };
            s.warn(format!(
                "{} {noun} above {PROC_FD_WARN_PCT:.0}% of the soft open-files limit (ulimit -n): {}: open() and accept() fail with EMFILE at the limit",
                over.len(),
                named.join(", ")
            ));
        }
    }

    if summary.is_empty()
        && let Some(cg) = cgroup
    {
        summary.push(format!("cgroup pids {} (no limit)", cg.current));
    }
    s.summary(summary.join(", "));
    s
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

    fn container_ctx() -> Context {
        Context {
            sys: SysInfo {
                container: true,
                ..Default::default()
            },
            interval: 1.0,
            count: 1,
        }
    }

    fn has(s: &Section, level: Level, needle: &str) -> bool {
        s.findings
            .iter()
            .any(|f| f.level == level && f.message.contains(needle))
    }

    fn fixture(tree: &str) -> FsSource {
        FsSource::new(format!(
            "{}/tests/fixtures/{tree}",
            env!("CARGO_MANIFEST_DIR")
        ))
    }

    // --- filesystems

    fn st(blocks: u64, bfree: u64, bavail: u64, files: u64, ffree: u64) -> FsStat {
        FsStat {
            frsize: 4096,
            blocks,
            bfree,
            bavail,
            files,
            ffree,
            favail: ffree,
            readonly: false,
        }
    }

    fn filesystems_at(src: &MemSource, ctx: &Context) -> Section {
        let mut c = Filesystems::default();
        c.sample(src, 0.0);
        c.sample(src, 1.0);
        c.evaluate(ctx)
    }

    fn filesystems(src: &MemSource) -> Section {
        filesystems_at(src, &ctx())
    }

    /// One xfs filesystem at `/data` with the given counts.
    fn single(stat: FsStat) -> Section {
        let src = MemSource::new().with(MOUNTS, "/dev/sdb1 /data xfs rw,relatime 0 0\n");
        src.set_statvfs("/data", stat);
        filesystems(&src)
    }

    #[test]
    fn filesystems_summary() {
        let src = MemSource::new().with(
            MOUNTS,
            "/dev/sda2 / xfs rw 0 0\n/dev/sda1 /boot xfs rw 0 0\n\
             tmpfs /run tmpfs rw,nosuid 0 0\n/dev/sdb1 /var/lib/app xfs rw 0 0\n",
        );
        src.set_statvfs("/", st(10_000_000, 5_500_000, 5_500_000, 1000, 950));
        src.set_statvfs("/var/lib/app", st(1_000_000, 200_000, 200_000, 1000, 900));
        src.set_statvfs("/boot", st(1000, 900, 900, 100, 90));
        src.set_statvfs("/run", st(1000, 990, 990, 100, 99));
        let s = filesystems(&src);
        assert_eq!(s.status, Status::Ok, "{:?}", s.findings);
        assert_eq!(
            s.summary,
            "/ 45% used (21 GiB free), 4 filesystems, fullest /var/lib/app 80%"
        );
        assert_eq!(s.details.len(), 4);
        assert_eq!(
            s.details[0],
            "/ xfs: size 38.1 GiB, used 45%, avail 21 GiB, inodes 5%"
        );
        assert_eq!(s.metrics["max_used_pct"], 80.0);
        assert_eq!(s.metrics["/.used_pct"], 45.0);
        assert_eq!(s.metrics["/var/lib/app.inodes_used_pct"], 10.0);
        assert_eq!(s.metrics["/run.used_pct"], 1.0);
    }

    #[test]
    fn fullest_part_omitted_when_root_is_fullest() {
        let src = MemSource::new().with(MOUNTS, "/dev/sda2 / ext4 rw 0 0\n");
        src.set_statvfs("/", st(1000, 500, 500, 10, 5));
        assert_eq!(
            filesystems(&src).summary,
            "/ 50% used (2 MiB free), 1 filesystem"
        );
    }

    #[test]
    fn root_reserve_counts_as_unavailable() {
        let s = single(st(1000, 100, 50, 1000, 1000));
        assert!((s.metrics["/data.used_pct"] - 900.0 * 100.0 / 950.0).abs() < 1e-9);
        assert_eq!(s.status, Status::Warn);
        assert!(
            has(&s, Level::Warn, "/data is 95% full"),
            "{:?}",
            s.findings
        );
    }

    #[test]
    fn btrfs_without_inodes() {
        let src = MemSource::new().with(MOUNTS, "/dev/sdc /srv btrfs rw 0 0\n");
        src.set_statvfs("/srv", st(1000, 500, 500, 0, 0));
        let s = filesystems(&src);
        assert!(s.details[0].ends_with("inodes -"), "{}", s.details[0]);
        assert!(!s.metrics.contains_key("/srv.inodes_used_pct"));
        assert_eq!(s.status, Status::Ok);
    }

    #[test]
    fn space_boundaries() {
        let at = |bfree| single(st(1000, bfree, bfree, 1000, 1000));
        assert_eq!(at(150).status, Status::Ok);
        let s = at(149);
        assert_eq!(s.status, Status::Warn);
        assert!(
            has(&s, Level::Warn, "/data is 86% full (596 KiB free)"),
            "{:?}",
            s.findings
        );
        assert_eq!(at(50).status, Status::Warn);
        let s = at(49);
        assert_eq!(s.status, Status::Crit);
        assert!(
            has(&s, Level::Crit, "/data is 96% full"),
            "{:?}",
            s.findings
        );
    }

    #[test]
    fn inode_boundaries() {
        let at = |ffree| single(st(1000, 1000, 1000, 1000, ffree));
        assert_eq!(at(150).status, Status::Ok);
        let s = at(149);
        assert_eq!(s.status, Status::Warn);
        assert!(
            has(&s, Level::Warn, "/data uses 86% of its inodes (149 free)"),
            "{:?}",
            s.findings
        );
        assert_eq!(at(50).status, Status::Warn);
        assert_eq!(at(49).status, Status::Crit);
    }

    #[test]
    fn pseudo_filesystems_are_skipped() {
        let src = MemSource::new().with(
            MOUNTS,
            "proc /proc proc rw 0 0\nsysfs /sys sysfs rw 0 0\n\
             cgroup2 /sys/fs/cgroup cgroup2 rw 0 0\n/dev/loop0 /snap/core/1 squashfs ro 0 0\n\
             /dev/sda1 / xfs rw 0 0\n",
        );
        for mp in ["/proc", "/sys", "/sys/fs/cgroup", "/snap/core/1"] {
            src.set_statvfs(mp, st(1000, 0, 0, 10, 0));
        }
        src.set_statvfs("/", st(1000, 500, 500, 10, 5));
        let s = filesystems(&src);
        assert_eq!(s.summary, "/ 50% used (2 MiB free), 1 filesystem");
        assert_eq!(s.status, Status::Ok);
    }

    #[test]
    fn zero_block_mount_is_ignored() {
        let src = MemSource::new().with(
            MOUNTS,
            "none /weird somefs rw 0 0\n/dev/sda1 / xfs rw 0 0\n",
        );
        src.set_statvfs("/weird", st(0, 0, 0, 0, 0));
        src.set_statvfs("/", st(1000, 500, 500, 10, 5));
        let s = filesystems(&src);
        assert!(s.summary.ends_with("1 filesystem"), "{}", s.summary);
        assert!(!s.metrics.contains_key("/weird.used_pct"));
    }

    #[test]
    fn escaped_mount_point() {
        let src = MemSource::new().with(MOUNTS, "/dev/sdb1 /mnt/my\\040disk ext4 rw 0 0\n");
        src.set_statvfs("/mnt/my disk", st(1000, 500, 500, 10, 5));
        let s = filesystems(&src);
        assert!(
            s.summary.starts_with("/mnt/my disk 50% used"),
            "{}",
            s.summary
        );
    }

    #[test]
    fn last_mount_per_mount_point_wins() {
        let src = MemSource::new().with(
            MOUNTS,
            "/dev/sda1 /data ext4 rw 0 0\n/dev/sdb1 /data xfs rw 0 0\n",
        );
        src.set_statvfs("/data", st(1000, 500, 500, 10, 5));
        let s = filesystems(&src);
        assert_eq!(s.details.len(), 1);
        assert!(s.details[0].starts_with("/data xfs:"), "{}", s.details[0]);
    }

    #[test]
    fn bind_mounts_are_deduplicated() {
        let src = fixture("linux-arm64");
        let mut c = Filesystems::default();
        c.sample(&src, 0.0);
        let s = c.evaluate(&container_ctx());
        let mounts: Vec<&str> = s
            .details
            .iter()
            .map(|d| d.split_whitespace().next().unwrap())
            .collect();
        assert_eq!(mounts, vec!["/dev", "/etc/hosts", "/dev/shm", "/"]);
        assert!(s.summary.contains("4 filesystems"), "{}", s.summary);

        let tmpfs = |mp: &str| format!("tmpfs {mp} tmpfs rw 0 0\n");
        let table: String = [
            "/run/secrets",
            "/etc/hostname",
            "/etc/resolv.conf",
            "/etc/hosts",
        ]
        .iter()
        .map(|m| tmpfs(m))
        .collect();
        let src = MemSource::new().with(MOUNTS, &table);
        for mp in [
            "/run/secrets",
            "/etc/hostname",
            "/etc/resolv.conf",
            "/etc/hosts",
        ] {
            src.set_statvfs(mp, st(99316, 99094, 99094, 819200, 818275));
        }
        let s = filesystems(&src);
        assert_eq!(s.details.len(), 1);
        assert!(
            s.details[0].starts_with("/etc/hosts tmpfs"),
            "{}",
            s.details[0]
        );
    }

    #[test]
    fn different_filesystems_are_kept() {
        let src = MemSource::new().with(
            MOUNTS,
            "/dev/mapper/root / xfs rw 0 0\n/dev/mapper/data /var/lib/app xfs rw 0 0\n",
        );
        src.set_statvfs("/", st(1000, 500, 500, 10, 5));
        src.set_statvfs("/var/lib/app", st(1000, 500, 500, 10, 5));
        assert_eq!(filesystems(&src).details.len(), 2);
    }

    fn ro(fstype: &str, mp: &str, ctx: &Context) -> Section {
        let src = MemSource::new().with(MOUNTS, &format!("/dev/x {mp} {fstype} ro,relatime 0 0\n"));
        src.set_statvfs(mp, st(1000, 500, 500, 10, 5));
        filesystems_at(&src, ctx)
    }

    #[test]
    fn readonly_ext4_note() {
        let s = ro("ext4", "/data", &ctx());
        assert_eq!(s.status, Status::Ok);
        assert!(
            has(&s, Level::Note, &format!("/data (ext4) {RO_NOTE}")),
            "{:?}",
            s.findings
        );
        // statvfs's ST_RDONLY counts too.
        let src = MemSource::new().with(MOUNTS, "/dev/x /data xfs rw 0 0\n");
        src.set_statvfs(
            "/data",
            FsStat {
                readonly: true,
                ..st(1000, 500, 500, 10, 5)
            },
        );
        assert!(has(&filesystems(&src), Level::Note, RO_NOTE));
    }

    #[test]
    fn readonly_tmpfs_no_note() {
        assert!(ro("tmpfs", "/data", &ctx()).findings.is_empty());
    }

    #[test]
    fn readonly_root_in_container_no_note() {
        assert!(ro("ext4", "/", &container_ctx()).findings.is_empty());
        // Outside a container a read-only root is noted.
        assert!(has(&ro("ext4", "/", &ctx()), Level::Note, RO_NOTE));
    }

    #[test]
    fn missing_mounts_is_skipped() {
        let s = filesystems(&MemSource::new());
        assert_eq!(s.status, Status::Skipped);
        assert!(s.summary.contains(MOUNTS), "{}", s.summary);
    }

    #[test]
    fn statvfs_failing_everywhere_is_skipped() {
        let src = MemSource::new().with(MOUNTS, "/dev/a / xfs rw 0 0\n/dev/b /data xfs rw 0 0\n");
        let s = filesystems(&src);
        assert_eq!(s.status, Status::Skipped);
        assert!(s.summary.contains("statvfs /"), "{}", s.summary);
    }

    #[test]
    fn statvfs_failing_for_one_mount() {
        let src = MemSource::new().with(MOUNTS, "/dev/a / xfs rw 0 0\n/dev/b /data xfs rw 0 0\n");
        src.set_statvfs("/", st(1000, 500, 500, 10, 5));
        let s = filesystems(&src);
        assert_ne!(s.status, Status::Skipped);
        assert_eq!(s.details.len(), 1);
    }

    #[test]
    fn only_pseudo_filesystems_is_skipped() {
        let src = MemSource::new().with(MOUNTS, "proc /proc proc rw 0 0\n");
        assert_eq!(filesystems(&src).status, Status::Skipped);
    }

    #[test]
    fn legacy_fixture_filesystems() {
        let src = fixture("linux-legacy");
        let mut c = Filesystems::default();
        c.sample(&src, 0.0);
        let s = c.evaluate(&ctx());
        assert_eq!(s.summary, "/ 90% used (5 GiB free), 4 filesystems");
        assert_eq!(s.status, Status::Warn);
        assert!(has(&s, Level::Warn, "/ is 90% full"), "{:?}", s.findings);
    }

    // --- limits

    fn limits_check() -> Limits {
        Limits {
            // Never collides with the fake pids used in these tests.
            own_pid: u32::MAX,
            ..Default::default()
        }
    }

    fn limits(src: &dyn Source) -> Section {
        let mut c = limits_check();
        c.sample(src, 0.0);
        c.sample(src, 1.0);
        c.evaluate(&ctx())
    }

    fn file_nr(allocated: u64, max: u64) -> Section {
        limits(&MemSource::new().with(FILE_NR, &format!("{allocated}\t0\t{max}\n")))
    }

    fn tasks(n: u64, pid_max: u64, threads_max: Option<u64>) -> Section {
        let src = MemSource::new()
            .with(LOADAVG, &format!("0.00 0.00 0.00 1/{n} 1\n"))
            .with(PID_MAX, &format!("{pid_max}\n"));
        if let Some(t) = threads_max {
            src.set(THREADS_MAX, &format!("{t}\n"));
        }
        limits(&src)
    }

    fn pids(current: u64, max: &str) -> MemSource {
        MemSource::new()
            .with(SELF_CGROUP, "0::/\n")
            .with("/sys/fs/cgroup/pids.current", &format!("{current}\n"))
            .with("/sys/fs/cgroup/pids.max", &format!("{max}\n"))
    }

    const LIMITS_HEADER: &str =
        "Limit                     Soft Limit           Hard Limit           Units     \n";

    fn add_proc(src: &MemSource, pid: u32, comm: &str, fds: u64, limit: u64) {
        src.set(
            &format!("/proc/{pid}/stat"),
            &format!("{pid} ({comm}) S 1 1 1 0 -1 0 0 0 0 0 1 1 0 0 20 0 1 0 5000 0 0"),
        );
        src.set(
            &format!("/proc/{pid}/limits"),
            &format!(
                "{LIMITS_HEADER}Max open files            {limit:<20} 4096                 files     \n"
            ),
        );
        for fd in 0..fds {
            src.set(&format!("/proc/{pid}/fd/{fd}"), "");
        }
    }

    #[test]
    fn limits_summary_legacy() {
        let s = limits(&fixture("linux-legacy"));
        assert_eq!(
            s.summary,
            "fds 3072/377212 (0.8%), tasks 287/32768, cgroup pids 212/4096, highest process app(4211) 240/256 fds"
        );
        assert_eq!(s.status, Status::Warn);
        assert_eq!(
            s.details[1],
            "tasks: 287, pid_max 32768 (0.9%), threads-max 30245 (0.9%)"
        );
        assert!(
            s.details[2].ends_with("in /sys/fs/cgroup/pids/system.slice/app.service"),
            "{}",
            s.details[2]
        );
        assert!((s.metrics["cgroup_pids_pct"] - 212.0 * 100.0 / 4096.0).abs() < 1e-9);
        assert_eq!(s.metrics["max_process_fd_pct"], 240.0 * 100.0 / 256.0);
        for k in ["file_nr_pct", "tasks_pct", "threads_pct"] {
            assert!(s.metrics.contains_key(k), "{k}");
        }
    }

    #[test]
    fn legacy_fixture_limits_warn() {
        let s = limits(&fixture("linux-legacy"));
        assert_eq!(s.status, Status::Warn);
        let warns: Vec<&str> = s
            .findings
            .iter()
            .filter(|f| f.level == Level::Warn)
            .map(|f| f.message.as_str())
            .collect();
        assert_eq!(warns.len(), 1, "{warns:?}");
        assert!(warns[0].contains("app(4211) 240/256"), "{}", warns[0]);
    }

    #[test]
    fn arm64_fixture_limits() {
        let s = limits(&fixture("linux-arm64"));
        assert_eq!(s.status, Status::Ok, "{:?}", s.findings);
        assert!(
            s.summary
                .starts_with("fds 1604/unlimited, tasks 212/4194304"),
            "{}",
            s.summary
        );
        assert!(!s.metrics.contains_key("cgroup_pids_pct"));
        assert!(
            s.details.iter().any(|d| d.ends_with("(no limit)")),
            "{:?}",
            s.details
        );
    }

    #[test]
    fn unlimited_file_max() {
        let s = file_nr(1604, 9223372036854775807);
        assert_eq!(s.summary, "fds 1604/unlimited");
        assert_eq!(s.status, Status::Ok);
        assert!(s.metrics["file_nr_pct"] < 1e-9);
    }

    #[test]
    fn file_nr_boundaries() {
        assert_eq!(file_nr(800, 1000).status, Status::Ok);
        let s = file_nr(801, 1000);
        assert_eq!(s.status, Status::Warn);
        assert!(has(
            &s,
            Level::Warn,
            "801 of 1000 system-wide file handles in use (80.1%)"
        ));
        assert_eq!(file_nr(900, 1000).status, Status::Warn);
        assert_eq!(file_nr(901, 1000).status, Status::Crit);
    }

    #[test]
    fn tasks_pid_max_boundaries() {
        assert_eq!(tasks(800, 1000, None).status, Status::Ok);
        let s = tasks(801, 1000, None);
        assert_eq!(s.status, Status::Warn);
        assert!(
            has(&s, Level::Warn, "80.1% of kernel.pid_max 1000"),
            "{:?}",
            s.findings
        );
        assert_eq!(s.summary, "tasks 801/1000");
        assert_eq!(tasks(900, 1000, None).status, Status::Warn);
        assert_eq!(tasks(901, 1000, None).status, Status::Crit);
    }

    #[test]
    fn tasks_threads_max_boundaries() {
        let at = |n| tasks(n, 4194304, Some(1000));
        assert_eq!(at(800).status, Status::Ok);
        let s = at(801);
        assert_eq!(s.status, Status::Warn);
        assert!(
            has(&s, Level::Warn, "kernel.threads-max 1000"),
            "{:?}",
            s.findings
        );
        assert_eq!(s.metrics["threads_pct"], 80.1);
        assert_eq!(at(900).status, Status::Warn);
        assert_eq!(at(901).status, Status::Crit);
    }

    #[test]
    fn cgroup_pids_boundaries() {
        let at = |n| limits(&pids(n, "1000"));
        assert_eq!(at(800).status, Status::Ok);
        let s = at(801);
        assert_eq!(s.status, Status::Warn);
        assert_eq!(s.summary, "cgroup pids 801/1000");
        assert_eq!(at(900).status, Status::Warn);
        assert_eq!(at(901).status, Status::Crit);
    }

    #[test]
    fn cgroup_pids_unlimited() {
        let src = pids(2, "max").with(FILE_NR, "1 0 100\n");
        let s = limits(&src);
        assert!(!s.metrics.contains_key("cgroup_pids_pct"));
        assert_eq!(s.details[1], "cgroup pids: 2 in /sys/fs/cgroup (no limit)");
        assert_eq!(s.summary, "fds 1/100 (1%)");
        // The cgroup alone still renders.
        let s = limits(&pids(2, "max"));
        assert_eq!(s.summary, "cgroup pids 2 (no limit)");
        assert_eq!(s.status, Status::Ok);
    }

    #[test]
    fn cgroup_v1_pids() {
        let dir = "/sys/fs/cgroup/pids/system.slice/app.service";
        let src = MemSource::new()
            .with(
                SELF_CGROUP,
                "10:memory:/x\n3:pids:/system.slice/app.service\n1:name=systemd:/y\n",
            )
            .with(&format!("{dir}/pids.current"), "212\n")
            .with(&format!("{dir}/pids.max"), "4096\n")
            // The v2 location must not be used when a v1 pids controller is listed.
            .with("/sys/fs/cgroup/pids.current", "999\n")
            .with("/sys/fs/cgroup/pids.max", "1000\n");
        let s = limits(&src);
        assert_eq!(s.summary, "cgroup pids 212/4096");
        assert_eq!(s.status, Status::Ok);
    }

    #[test]
    fn cgroup_v1_container_path_walks_up() {
        // Inside a container the host path is listed, but the mount root is our cgroup.
        let src = MemSource::new()
            .with(SELF_CGROUP, "3:pids:/docker/abc\n")
            .with("/sys/fs/cgroup/pids/pids.current", "5\n")
            .with("/sys/fs/cgroup/pids/pids.max", "100\n");
        assert_eq!(limits(&src).summary, "cgroup pids 5/100");
    }

    #[test]
    fn cgroup_v2_nested_pids() {
        let src = MemSource::new()
            .with(SELF_CGROUP, "0::/a/b\n")
            .with("/sys/fs/cgroup/a/b/pids.current", "3\n")
            .with("/sys/fs/cgroup/a/b/pids.max", "max\n")
            .with("/sys/fs/cgroup/a/pids.current", "950\n")
            .with("/sys/fs/cgroup/a/pids.max", "1000\n");
        let s = limits(&src);
        assert_eq!(s.metrics["cgroup_pids_pct"], 95.0);
        assert_eq!(s.status, Status::Crit);
        assert!(
            has(&s, Level::Crit, "in /sys/fs/cgroup/a:"),
            "{:?}",
            s.findings
        );
    }

    #[test]
    fn process_fd_boundaries() {
        let at = |fds| {
            let src = MemSource::new();
            add_proc(&src, 4211, "app", fds, 1000);
            limits(&src)
        };
        let s = at(900);
        assert_eq!(s.status, Status::Ok);
        assert_eq!(s.summary, "highest process app(4211) 900/1000 fds");
        assert_eq!(s.metrics["max_process_fd_pct"], 90.0);
        let s = at(901);
        assert_eq!(s.status, Status::Warn);
        assert!(
            has(&s, Level::Warn, "app(4211) 901/1000"),
            "{:?}",
            s.findings
        );
    }

    #[test]
    fn at_most_three_processes_named() {
        let src = MemSource::new();
        for (i, pid) in (100..105).enumerate() {
            add_proc(&src, pid, &format!("p{i}"), 95 + i as u64, 100);
        }
        add_proc(&src, 200, "calm", 10, 100);
        let s = limits(&src);
        assert_eq!(s.status, Status::Warn);
        let f = &s.findings[0].message;
        assert!(f.starts_with("5 processes are above 90%"), "{f}");
        assert!(
            f.contains("p4(104) 99/100, p3(103) 98/100, p2(102) 97/100, +2 more"),
            "{f}"
        );
        assert!(!f.contains("p1(101)"), "{f}");
        assert!(
            s.summary.ends_with("highest process p4(104) 99/100 fds"),
            "{}",
            s.summary
        );
    }

    #[test]
    fn unreadable_fd_dirs_are_ignored() {
        let src = MemSource::new();
        add_proc(&src, 11, "mine", 3, 1024);
        // pid 10: limits and stat readable, fd directory not (another user's process).
        add_proc(&src, 10, "theirs", 0, 16);
        // pid 12: vanished between readdir and reading limits.
        src.set("/proc/12/fd/0", "");
        let s = limits(&src);
        assert_eq!(s.status, Status::Ok);
        assert!(s.findings.is_empty(), "{:?}", s.findings);
        assert_eq!(s.summary, "highest process mine(11) 3/1024 fds");
        assert!(
            s.details[0].starts_with("open files: 1 process readable"),
            "{}",
            s.details[0]
        );
    }

    #[test]
    fn unlimited_soft_limit_is_ignored() {
        let src = MemSource::new()
            .with(
                "/proc/5/limits",
                &format!("{LIMITS_HEADER}Max open files            unlimited            unlimited            files\n"),
            )
            .with("/proc/5/fd/0", "")
            .with(FILE_NR, "1 0 100\n");
        let s = limits(&src);
        assert!(!s.metrics.contains_key("max_process_fd_pct"));
        assert_eq!(s.status, Status::Ok);
    }

    #[test]
    fn nothing_readable_is_skipped() {
        let s = limits(&MemSource::new());
        assert_eq!(s.status, Status::Skipped);
        assert!(s.summary.contains(FILE_NR), "{}", s.summary);
    }

    #[test]
    fn tasks_without_limits_do_not_count() {
        let s = limits(&MemSource::new().with(LOADAVG, "0 0 0 1/100 1\n"));
        assert_eq!(s.status, Status::Skipped);
    }

    #[test]
    fn only_file_nr_readable() {
        let s = file_nr(3072, 377212);
        assert_eq!(s.status, Status::Ok);
        assert_eq!(s.summary, "fds 3072/377212 (0.8%)");
        assert!(!s.metrics.contains_key("tasks_pct"));
    }

    #[test]
    fn helpers() {
        assert_eq!(pct(1, 0), 0.0);
        assert_eq!(pct_ceil(851, 1000), 86);
        assert_eq!(pct_ceil(850, 1000), 85);
        assert_eq!(pct_ceil(1, 0), 0);
        assert_eq!(pct1(80.0), "80");
        assert_eq!(pct1(0.814), "0.8");
    }
}
