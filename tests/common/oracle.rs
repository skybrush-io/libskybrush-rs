//! Access to the test oracle in `tests/oracle`.

use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};

use serde_json::Value;

use super::compare::Tolerances;

/// The path of a file or directory of the oracle.
pub fn path(relative: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/oracle")
        .join(relative)
}

pub fn read_bytes(relative: &str) -> Vec<u8> {
    let path = path(relative);
    fs::read(&path).unwrap_or_else(|err| panic!("cannot read {}: {err}", path.display()))
}

pub fn read_json(relative: &str) -> Value {
    let bytes = read_bytes(relative);
    serde_json::from_slice(&bytes).unwrap_or_else(|err| panic!("cannot parse {relative}: {err}"))
}

/// Reads `tolerances.json` of a directory of the oracle.
pub fn read_tolerances(dir: &str) -> Tolerances {
    Tolerances::from_json(&read_json(&format!("{dir}/tolerances.json")))
}

/// The names of the golden traces of a directory (without `.json`), sorted,
/// leaving out the given names.
pub fn trace_names(dir: &str, except: &[&str]) -> Vec<String> {
    let path = path(dir);
    let entries =
        fs::read_dir(&path).unwrap_or_else(|err| panic!("cannot list {}: {err}", path.display()));
    let mut names: Vec<String> = entries
        .map(|entry| entry.expect("directory entry").path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "json"))
        .map(|path| {
            path.file_stem()
                .expect("file name")
                .to_string_lossy()
                .into_owned()
        })
        .filter(|name| !except.contains(&name.as_str()))
        .collect();
    names.sort();
    assert!(!names.is_empty(), "no traces in {}", path.display());
    names
}

/// Decodes a hexadecimal string, as used for the byte strings of the oracle.
pub fn decode_hex(hex: &str) -> Vec<u8> {
    assert!(
        hex.len().is_multiple_of(2),
        "odd number of hex digits: {hex}"
    );
    (0..hex.len())
        .step_by(2)
        .map(|i| {
            u8::from_str_radix(&hex[i..i + 2], 16).unwrap_or_else(|_| panic!("not hex: {hex}"))
        })
        .collect()
}

/// Encodes bytes as a hexadecimal string, as used for the byte strings of the
/// oracle.
pub fn encode_hex(bytes: &[u8]) -> String {
    bytes.iter().fold(String::new(), |mut hex, byte| {
        let _ = write!(hex, "{byte:02x}");
        hex
    })
}

/// Asserts that a golden file has the version of the format that the harness
/// understands.
pub fn assert_version(name: &str, golden: &Value, version: u64) {
    assert_eq!(
        golden["version"].as_u64(),
        Some(version),
        "{name}: unsupported version of the oracle format"
    );
}
