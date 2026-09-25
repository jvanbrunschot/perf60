//! Loading eBPF objects and attaching their raw-tracepoint programs (feature `deep`).
//!
//! Raw tracepoints attach by name through bpf(2): no tracefs mount is needed (containers and
//! minimal hosts often have none), and they are cheaper than classic tracepoints. Programs that
//! read kernel structs get member offsets from the kernel's BTF (see [`super::btf`]).

use aya::maps::{Array, HashMap, MapData, PerCpuArray};
use aya::programs::RawTracePoint;
use aya::{Ebpf, EbpfLoader, Pod};

/// A loaded object with its programs attached. Dropping it detaches everything.
pub struct Probe {
    ebpf: Ebpf,
}

impl Probe {
    /// Load `object` and attach `(program, raw tracepoint)` for each entry.
    pub fn attach(object: &[u8], programs: &[(&str, &str)]) -> Result<Probe, String> {
        Self::attach_with(object, programs, &[])
    }

    /// Like [`Probe::attach`], first setting `u64` config globals (e.g. BTF member offsets)
    /// declared in the program as `#[unsafe(no_mangle)] static NAME: u64`.
    pub fn attach_with(
        object: &[u8],
        programs: &[(&str, &str)],
        globals: &[(&str, u64)],
    ) -> Result<Probe, String> {
        super::caps::require_bpf()?;
        raise_memlock_limit();
        let mut loader = EbpfLoader::new();
        for (name, value) in globals {
            loader.override_global(name, value, true);
        }
        let mut ebpf = loader
            .load(object)
            .map_err(|e| format!("eBPF load failed: {e}"))?;
        for (name, tracepoint) in programs {
            let prog: &mut RawTracePoint = ebpf
                .program_mut(name)
                .ok_or_else(|| format!("eBPF program {name} missing from object"))?
                .try_into()
                .map_err(|e: aya::programs::ProgramError| e.to_string())?;
            prog.load()
                .map_err(|e| format!("eBPF verifier rejected {name}: {e}"))?;
            prog.attach(tracepoint)
                .map_err(|e| format!("attaching raw tracepoint {tracepoint}: {e}"))?;
        }
        Ok(Probe { ebpf })
    }

    pub fn hash_map<K: Pod, V: Pod>(&self, name: &str) -> Result<Vec<(K, V)>, String> {
        let map = self
            .ebpf
            .map(name)
            .ok_or_else(|| format!("map {name} missing"))?;
        let m: HashMap<&MapData, K, V> = HashMap::try_from(map).map_err(|e| e.to_string())?;
        Ok(m.iter().filter_map(Result::ok).collect())
    }

    /// Sum over CPUs of each of the first `n` slots of a per-CPU u64 array.
    pub fn per_cpu_sums(&self, name: &str, n: u32) -> Result<Vec<u64>, String> {
        let map = self
            .ebpf
            .map(name)
            .ok_or_else(|| format!("map {name} missing"))?;
        let a: PerCpuArray<&MapData, u64> =
            PerCpuArray::try_from(map).map_err(|e| e.to_string())?;
        (0..n)
            .map(|i| {
                a.get(&i, 0)
                    .map(|v| v.iter().sum())
                    .map_err(|e| e.to_string())
            })
            .collect()
    }

    /// The first `n` slots of a u64 array.
    pub fn array(&self, name: &str, n: u32) -> Result<Vec<u64>, String> {
        let map = self
            .ebpf
            .map(name)
            .ok_or_else(|| format!("map {name} missing"))?;
        let a: Array<&MapData, u64> = Array::try_from(map).map_err(|e| e.to_string())?;
        (0..n)
            .map(|i| a.get(&i, 0).map_err(|e| e.to_string()))
            .collect()
    }
}

/// Kernels before 5.11 charge eBPF maps against RLIMIT_MEMLOCK; lift it for this process.
fn raise_memlock_limit() {
    let lim = libc::rlimit {
        rlim_cur: libc::RLIM_INFINITY,
        rlim_max: libc::RLIM_INFINITY,
    };
    // SAFETY: plain syscall with a valid struct; failure is harmless (newer kernels ignore it).
    unsafe {
        libc::setrlimit(libc::RLIMIT_MEMLOCK, &lim);
    }
}
