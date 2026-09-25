//! `/proc/net/dev`: two header lines, then one line per interface:
//! `  eth0: rx_bytes rx_packets rx_errs rx_drop rx_fifo rx_frame rx_compressed rx_multicast
//! tx_bytes tx_packets tx_errs tx_drop tx_fifo tx_colls tx_carrier tx_compressed`.
//! Large counters can run straight into the colon (`eth0:123456 ...`).

use super::{Result, err};

#[derive(Debug, Clone, Default, PartialEq)]
pub struct IfStats {
    pub name: String,
    pub rx_bytes: u64,
    pub rx_packets: u64,
    pub rx_errs: u64,
    pub rx_drop: u64,
    pub rx_fifo: u64,
    pub rx_frame: u64,
    pub rx_compressed: u64,
    pub rx_multicast: u64,
    pub tx_bytes: u64,
    pub tx_packets: u64,
    pub tx_errs: u64,
    pub tx_drop: u64,
    pub tx_fifo: u64,
    pub tx_colls: u64,
    pub tx_carrier: u64,
    pub tx_compressed: u64,
}

pub fn parse(input: &str) -> Result<Vec<IfStats>> {
    let mut out = Vec::new();
    for line in input.lines() {
        // Header lines use `|` separators and have no colon.
        let Some((name, rest)) = line.split_once(':') else {
            continue;
        };
        let name = name.trim();
        let v: Vec<u64> = match rest.split_whitespace().map(str::parse).collect() {
            Ok(v) => v,
            Err(_) => return err(format!("net/dev: bad counter for {name}")),
        };
        if v.len() < 16 {
            return err(format!(
                "net/dev: {name} has {} counters, expected 16",
                v.len()
            ));
        }
        out.push(IfStats {
            name: name.to_owned(),
            rx_bytes: v[0],
            rx_packets: v[1],
            rx_errs: v[2],
            rx_drop: v[3],
            rx_fifo: v[4],
            rx_frame: v[5],
            rx_compressed: v[6],
            rx_multicast: v[7],
            tx_bytes: v[8],
            tx_packets: v[9],
            tx_errs: v[10],
            tx_drop: v[11],
            tx_fifo: v[12],
            tx_colls: v[13],
            tx_carrier: v[14],
            tx_compressed: v[15],
        });
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_fixture() {
        let v = parse(include_str!(
            "../../tests/fixtures/linux-arm64/proc/net/dev"
        ))
        .unwrap();
        let names: Vec<&str> = v.iter().map(|i| i.name.as_str()).collect();
        assert_eq!(names, ["lo", "eth0"]);
        let eth0 = &v[1];
        assert_eq!(eth0.rx_bytes, 90);
        assert_eq!(eth0.rx_packets, 1);
        assert_eq!(eth0.tx_bytes, 132);
        assert_eq!(eth0.tx_packets, 2);
    }

    #[test]
    fn handles_counter_touching_colon_and_all_fields() {
        let input = "Inter-|   Receive  |  Transmit\n face |bytes packets|bytes\n\
                     enp1s0:12345678901 2 3 4 5 6 7 8 9 10 11 12 13 14 15 16\n";
        let v = parse(input).unwrap();
        assert_eq!(v.len(), 1);
        let i = &v[0];
        assert_eq!(i.name, "enp1s0");
        assert_eq!(i.rx_bytes, 12345678901);
        assert_eq!(
            (i.rx_errs, i.rx_drop, i.rx_multicast),
            (3, 4, 8),
            "rx fields"
        );
        assert_eq!(
            (i.tx_bytes, i.tx_errs, i.tx_drop, i.tx_compressed),
            (9, 11, 12, 16),
            "tx fields"
        );
    }

    #[test]
    fn rejects_short_or_garbled_lines() {
        assert!(parse("eth0: 1 2 3\n").is_err());
        assert!(parse("eth0: 1 2 x 4 5 6 7 8 9 10 11 12 13 14 15 16\n").is_err());
        assert_eq!(parse("Inter-| Receive\n face |bytes\n").unwrap(), vec![]);
    }
}
