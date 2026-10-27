//! Lists of known failures that may only shrink.
//!
//! Each runner checks a set of items (a conformance case, a section of a
//! trace...) and compares the failing items with the list of known failures
//! in `tests/known_failures/<runner>.txt`. The test fails if an item fails
//! that is not on the list, if an item on the list passes (remove it from the
//! list), or if the list names an item that does not exist.
//!
//! Run the tests with `UPDATE_KNOWN_FAILURES=1` to rewrite the lists from the
//! current results, and review the changes before committing them.

use std::collections::BTreeSet;
use std::fmt::Write as _;
use std::path::PathBuf;
use std::{env, fs};

/// Maximum number of differences to report per item.
const MAX_REPORTED_DIFFERENCES: usize = 20;

/// Why an item failed.
#[derive(Debug)]
pub enum Failure {
    /// The crate cannot compute the item yet.
    Unimplemented,
    /// The results differ from the golden ones.
    Differences(Vec<String>),
}

/// Collects the results of the items of a runner.
pub struct Ratchet {
    name: &'static str,
    results: Vec<(String, Result<(), Failure>)>,
}

impl Ratchet {
    pub fn new(name: &'static str) -> Self {
        Self {
            name,
            results: Vec::new(),
        }
    }

    pub fn record(&mut self, id: String, result: Result<(), Failure>) {
        assert!(
            self.results.iter().all(|(existing, _)| *existing != id),
            "{}: duplicate item {id}",
            self.name
        );
        self.results.push((id, result));
    }

    /// Records the differences found by a comparison; no differences is a pass.
    pub fn record_differences(&mut self, id: String, differences: Vec<String>) {
        let result = if differences.is_empty() {
            Ok(())
        } else {
            Err(Failure::Differences(differences))
        };
        self.record(id, result);
    }

    /// Compares the results with the list of known failures and panics with a
    /// report if they do not agree.
    pub fn finish(self) {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/known_failures")
            .join(format!("{}.txt", self.name));
        let failing: BTreeSet<&str> = self
            .results
            .iter()
            .filter(|(_, result)| result.is_err())
            .map(|(id, _)| id.as_str())
            .collect();
        let unimplemented = self
            .results
            .iter()
            .filter(|(_, result)| matches!(result, Err(Failure::Unimplemented)))
            .count();

        eprintln!(
            "{}: {} of {} items pass; {} fail, {} of them unimplemented",
            self.name,
            self.results.len() - failing.len(),
            self.results.len(),
            failing.len(),
            unimplemented
        );

        if env::var_os("UPDATE_KNOWN_FAILURES").is_some() {
            let mut contents = format!(
                "# Known failures of the {} runner, one item per line.\n\
                 # The list may only shrink; see tests/common/ratchet.rs.\n",
                self.name
            );
            for id in &failing {
                writeln!(contents, "{id}").expect("writing to a string");
            }
            fs::create_dir_all(path.parent().expect("parent directory"))
                .expect("creating tests/known_failures");
            fs::write(&path, contents)
                .unwrap_or_else(|err| panic!("cannot write {}: {err}", path.display()));
            eprintln!("{}: updated {}", self.name, path.display());
            return;
        }

        let known = read_list(&path);
        let ids: BTreeSet<&str> = self.results.iter().map(|(id, _)| id.as_str()).collect();
        let mut report = String::new();

        for (id, result) in &self.results {
            match result {
                Err(failure) if !known.contains(id.as_str()) => {
                    writeln!(report, "FAIL {id} (not a known failure)").expect("write");
                    match failure {
                        Failure::Unimplemented => {
                            writeln!(report, "  not implemented").expect("write");
                        }
                        Failure::Differences(differences) => {
                            for difference in differences.iter().take(MAX_REPORTED_DIFFERENCES) {
                                writeln!(report, "  {difference}").expect("write");
                            }
                            if differences.len() > MAX_REPORTED_DIFFERENCES {
                                let more = differences.len() - MAX_REPORTED_DIFFERENCES;
                                writeln!(report, "  ... and {more} more").expect("write");
                            }
                        }
                    }
                }
                Ok(()) if known.contains(id.as_str()) => {
                    writeln!(
                        report,
                        "PASS {id} (a known failure; remove it from the list)"
                    )
                    .expect("write");
                }
                _ => {}
            }
        }
        for id in known.iter().filter(|id| !ids.contains(id.as_str())) {
            writeln!(report, "STALE {id} (no such item; remove it from the list)").expect("write");
        }

        assert!(
            report.is_empty(),
            "{}: the results do not agree with {}\n{report}",
            self.name,
            path.display()
        );
    }
}

fn read_list(path: &PathBuf) -> BTreeSet<String> {
    let contents = match fs::read_to_string(path) {
        Ok(contents) => contents,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(err) => panic!("cannot read {}: {err}", path.display()),
    };
    contents
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .map(str::to_owned)
        .collect()
}
