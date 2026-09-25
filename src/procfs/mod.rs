//! Pure parsers for kernel text interfaces. No I/O here: every function takes the file content
//! as `&str` so it can be tested against fixtures on any OS.

pub mod diskstats;
pub mod loadavg;
pub mod meminfo;
pub mod pid_stat;
pub mod pressure;
pub mod stat;
pub mod system;

/// Error from a parser: the input did not have the expected shape.
#[derive(Debug, Clone, PartialEq)]
pub struct ParseError(pub String);

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

pub type Result<T> = std::result::Result<T, ParseError>;

pub(crate) fn err<T>(msg: impl Into<String>) -> Result<T> {
    Err(ParseError(msg.into()))
}

/// Parse `key: value` / `key value` lines into pairs (keys trimmed, values trimmed).
pub fn key_values(input: &str, sep: char) -> Vec<(&str, &str)> {
    input
        .lines()
        .filter_map(|l| l.split_once(sep))
        .map(|(k, v)| (k.trim(), v.trim()))
        .collect()
}
