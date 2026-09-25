//! Minimal reader for the kernel's BTF (`/sys/kernel/btf/vmlinux`): find the byte offset of a
//! struct member, so raw-tracepoint programs can read kernel structs (e.g. `task_struct.pid`)
//! with `bpf_probe_read_kernel` on any kernel layout. Pure, no dependencies.
//!
//! Format: `include/uapi/linux/btf.h`. A header, a type section of variable-length records
//! (type ids count from 1 in order) and a NUL-separated string section.

const MAGIC: u16 = 0xeb9f;
const HEADER_MIN: usize = 24;
const TYPE_LEN: usize = 12;

const KIND_INT: u8 = 1;
const KIND_ARRAY: u8 = 3;
const KIND_STRUCT: u8 = 4;
const KIND_UNION: u8 = 5;
const KIND_ENUM: u8 = 6;
const KIND_TYPEDEF: u8 = 8;
const KIND_VOLATILE: u8 = 9;
const KIND_CONST: u8 = 10;
const KIND_RESTRICT: u8 = 11;
const KIND_FUNC_PROTO: u8 = 13;
const KIND_VAR: u8 = 14;
const KIND_DATASEC: u8 = 15;
const KIND_DECL_TAG: u8 = 17;
const KIND_TYPE_TAG: u8 = 18;
const KIND_ENUM64: u8 = 19;

#[derive(Clone, Copy, Debug)]
struct Type {
    kind: u8,
    kind_flag: bool,
    vlen: usize,
    name_off: u32,
    /// `type` for modifiers/typedefs (size for structs, unused here).
    size_or_type: u32,
    /// Byte position of the record's variable part (members) in the type section.
    extra: usize,
}

pub struct Btf<'a> {
    types: Vec<Type>,
    type_sec: &'a [u8],
    strings: &'a [u8],
}

fn u16_at(b: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_le_bytes(b.get(at..at + 2)?.try_into().ok()?))
}

fn u32_at(b: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(b.get(at..at + 4)?.try_into().ok()?))
}

impl<'a> Btf<'a> {
    pub fn parse(data: &'a [u8]) -> Result<Btf<'a>, String> {
        let bad = || "not a little-endian BTF blob".to_owned();
        if u16_at(data, 0) != Some(MAGIC) {
            return Err(bad());
        }
        let hdr_len = u32_at(data, 4).ok_or_else(bad)? as usize;
        if hdr_len < HEADER_MIN {
            return Err(bad());
        }
        let field = |i: usize| u32_at(data, 8 + 4 * i).map(|v| v as usize);
        let (type_off, type_len, str_off, str_len) = (
            field(0).ok_or_else(bad)?,
            field(1).ok_or_else(bad)?,
            field(2).ok_or_else(bad)?,
            field(3).ok_or_else(bad)?,
        );
        let section = |off: usize, len: usize| {
            data.get(hdr_len + off..hdr_len + off + len)
                .ok_or_else(|| "BTF section out of bounds".to_owned())
        };
        let type_sec = section(type_off, type_len)?;
        let strings = section(str_off, str_len)?;

        let mut types = Vec::new();
        let mut pos = 0;
        while pos + TYPE_LEN <= type_sec.len() {
            let name_off = u32_at(type_sec, pos).unwrap();
            let info = u32_at(type_sec, pos + 4).unwrap();
            let size_or_type = u32_at(type_sec, pos + 8).unwrap();
            let kind = ((info >> 24) & 0x1f) as u8;
            let vlen = (info & 0xffff) as usize;
            let extra = pos + TYPE_LEN;
            let extra_len = match kind {
                KIND_INT | KIND_VAR | KIND_DECL_TAG => 4,
                KIND_ARRAY => 12,
                KIND_STRUCT | KIND_UNION | KIND_DATASEC | KIND_ENUM64 => 12 * vlen,
                KIND_ENUM | KIND_FUNC_PROTO => 8 * vlen,
                0..=19 => 0,
                k => return Err(format!("unknown BTF kind {k}")),
            };
            types.push(Type {
                kind,
                kind_flag: info >> 31 == 1,
                vlen,
                name_off,
                size_or_type,
                extra,
            });
            pos = extra + extra_len;
        }
        if pos != type_sec.len() {
            return Err("truncated BTF type section".into());
        }
        Ok(Btf {
            types,
            type_sec,
            strings,
        })
    }

    fn name(&self, off: u32) -> &[u8] {
        let s = self.strings.get(off as usize..).unwrap_or(&[]);
        &s[..s.iter().position(|&c| c == 0).unwrap_or(s.len())]
    }

    /// Type by id (1-based; 0 is `void`).
    fn by_id(&self, id: u32) -> Option<&Type> {
        self.types.get((id as usize).checked_sub(1)?)
    }

    /// Follow typedefs and qualifiers to the underlying type.
    fn resolve(&self, mut id: u32) -> Option<&Type> {
        for _ in 0..32 {
            let t = self.by_id(id)?;
            match t.kind {
                KIND_TYPEDEF | KIND_VOLATILE | KIND_CONST | KIND_RESTRICT | KIND_TYPE_TAG => {
                    id = t.size_or_type
                }
                _ => return Some(t),
            }
        }
        None
    }

    /// `(name_off, type_id, bit_offset)` of each member of a struct/union.
    fn members(&self, t: &Type) -> impl Iterator<Item = (u32, u32, u32)> + '_ {
        let (extra, vlen, bitfields) = (t.extra, t.vlen, t.kind_flag);
        (0..vlen).map(move |i| {
            let at = extra + 12 * i;
            let off = u32_at(self.type_sec, at + 8).unwrap_or(0);
            (
                u32_at(self.type_sec, at).unwrap_or(0),
                u32_at(self.type_sec, at + 4).unwrap_or(0),
                // With kind_flag, the top 8 bits hold the bitfield size.
                if bitfields { off & 0x00ff_ffff } else { off },
            )
        })
    }

    /// Bit offset of `member` in `t`, looking into anonymous struct/union members.
    fn find_in(&self, t: &Type, member: &[u8], depth: u32) -> Option<u32> {
        if depth > 8 {
            return None;
        }
        for (name_off, ty, bits) in self.members(t) {
            if name_off != 0 && self.name(name_off) == member {
                return Some(bits);
            }
            if name_off == 0
                && let Some(inner) = self.resolve(ty)
                && matches!(inner.kind, KIND_STRUCT | KIND_UNION)
                && let Some(b) = self.find_in(inner, member, depth + 1)
            {
                return Some(bits + b);
            }
        }
        None
    }

    /// Byte offset of `member` in `struct <name>` (the first definition with members).
    /// `None` when the struct or member doesn't exist, or the member is a bitfield.
    pub fn member_offset(&self, name: &str, member: &str) -> Option<u32> {
        self.types
            .iter()
            .filter(|t| {
                t.kind == KIND_STRUCT && t.vlen > 0 && self.name(t.name_off) == name.as_bytes()
            })
            .find_map(|t| self.find_in(t, member.as_bytes(), 0))
            .filter(|bits| bits % 8 == 0)
            .map(|bits| bits / 8)
    }
}

/// Offsets of `(struct, member)` pairs from the running kernel's BTF.
#[cfg(target_os = "linux")]
pub fn kernel_offsets(wanted: &[(&str, &str)]) -> Result<Vec<u32>, String> {
    let data = std::fs::read("/sys/kernel/btf/vmlinux").map_err(|e| {
        format!(
            "kernel BTF not available (/sys/kernel/btf/vmlinux: {e}); needs CONFIG_DEBUG_INFO_BTF"
        )
    })?;
    let btf = Btf::parse(&data)?;
    wanted
        .iter()
        .map(|(s, m)| {
            btf.member_offset(s, m)
                .ok_or_else(|| format!("kernel BTF has no {s}.{m}"))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Tiny BTF builder for tests.
    #[derive(Default)]
    struct Builder {
        types: Vec<u8>,
        strings: Vec<u8>,
    }

    impl Builder {
        fn new() -> Self {
            Builder {
                types: Vec::new(),
                strings: vec![0],
            }
        }
        fn s(&mut self, name: &str) -> u32 {
            if name.is_empty() {
                return 0;
            }
            let off = self.strings.len() as u32;
            self.strings.extend_from_slice(name.as_bytes());
            self.strings.push(0);
            off
        }
        fn rec(&mut self, name: &str, kind: u8, kind_flag: bool, vlen: u32, size_or_type: u32) {
            let n = self.s(name);
            let info = ((kind_flag as u32) << 31) | ((kind as u32) << 24) | vlen;
            for v in [n, info, size_or_type] {
                self.types.extend_from_slice(&v.to_le_bytes());
            }
        }
        fn int(&mut self, name: &str) {
            self.rec(name, KIND_INT, false, 0, 4);
            self.types.extend_from_slice(&(32u32).to_le_bytes());
        }
        fn composite(
            &mut self,
            kind: u8,
            name: &str,
            bitfields: bool,
            members: &[(&str, u32, u32)],
        ) {
            self.rec(name, kind, bitfields, members.len() as u32, 64);
            for (m, ty, off) in members {
                let n = self.s(m);
                for v in [n, *ty, *off] {
                    self.types.extend_from_slice(&v.to_le_bytes());
                }
            }
        }
        fn build(self) -> Vec<u8> {
            let mut out = Vec::new();
            out.extend_from_slice(&MAGIC.to_le_bytes());
            out.extend_from_slice(&[1, 0]);
            for v in [
                24u32,
                0,
                self.types.len() as u32,
                self.types.len() as u32,
                self.strings.len() as u32,
            ] {
                out.extend_from_slice(&v.to_le_bytes());
            }
            out.extend(self.types);
            out.extend(self.strings);
            out
        }
    }

    fn sample() -> Vec<u8> {
        let mut b = Builder::new();
        b.int("int"); // 1
        b.composite(KIND_STRUCT, "inner", false, &[("a", 1, 0), ("b", 1, 32)]); // 2
        b.composite(KIND_UNION, "", false, &[("x", 1, 0), ("y", 2, 0)]); // 3
        b.rec("", KIND_CONST, false, 0, 3); // 4: const anon union
        b.composite(
            KIND_STRUCT,
            "task_struct",
            false,
            &[("state", 1, 0), ("", 4, 64), ("pid", 1, 128)],
        ); // 5
        b.composite(
            KIND_STRUCT,
            "flags",
            true,
            &[("f1", 1, (1 << 24) | 3), ("f2", 1, (8 << 24) | 8)],
        ); // 6
        b.build()
    }

    #[test]
    fn member_offsets() {
        let data = sample();
        let btf = Btf::parse(&data).unwrap();
        assert_eq!(btf.member_offset("task_struct", "state"), Some(0));
        assert_eq!(btf.member_offset("task_struct", "pid"), Some(16));
        // Through the anonymous (const) union at byte 8.
        assert_eq!(btf.member_offset("task_struct", "x"), Some(8));
        // `b` lives in the *named* member `y`: not a direct member.
        assert_eq!(btf.member_offset("task_struct", "b"), None);
        assert_eq!(btf.member_offset("inner", "b"), Some(4));
        assert_eq!(btf.member_offset("nope", "pid"), None);
        // Bitfields: f1 at bit 3 is not byte-addressable; f2 at bit 8 is byte 1.
        assert_eq!(btf.member_offset("flags", "f1"), None);
        assert_eq!(btf.member_offset("flags", "f2"), Some(1));
    }

    #[test]
    fn rejects_garbage() {
        assert!(Btf::parse(b"").is_err());
        assert!(Btf::parse(&[0x9f, 0xeb, 1, 0, 24, 0, 0, 0]).is_err());
        let mut data = sample();
        data.truncate(data.len() - 20);
        assert!(Btf::parse(&data).is_err());
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn running_kernel() {
        if !std::path::Path::new("/sys/kernel/btf/vmlinux").exists() {
            return;
        }
        let o = kernel_offsets(&[("task_struct", "pid"), ("task_struct", "tgid")]).unwrap();
        assert!(o[0] > 0 && o[1] == o[0] + 4, "{o:?}");
    }
}
