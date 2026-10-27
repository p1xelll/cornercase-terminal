use std::collections::{BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use super::git::{self, Deadline};
use crate::error::Result;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Work {
    pub(crate) head: Option<String>,
    pub(crate) files: BTreeSet<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Parted {
    pub(crate) first: BTreeSet<String>,
    pub(crate) second: BTreeSet<String>,
}

pub(crate) type Known = HashMap<(String, String), Arc<Parted>>;

pub(crate) type Pair = ((PathBuf, PathBuf), Vec<String>);

#[derive(Debug, Default)]
pub(crate) struct Scan {
    pub(crate) pairs: Vec<Pair>,
    pub(crate) known: Known,
}

pub(crate) fn scan(dirs: &[PathBuf], known: &Known) -> Scan {
    let _deadline = Deadline::arm();
    let work: Vec<_> = dirs.iter().map(|dir| (dir, work_of(dir))).collect();
    let mut scan = Scan::default();
    for (i, (first_dir, first)) in work.iter().enumerate() {
        let Ok(first) = first else { continue };
        for (second_dir, second) in &work[i + 1..] {
            let Ok(second) = second else { continue };
            if first_dir != second_dir {
                scan.add_pair((first_dir, first), (second_dir, second), known);
            }
        }
    }
    scan
}

impl Scan {
    fn add_pair(&mut self, first: (&Path, &Work), second: (&Path, &Work), known: &Known) {
        let (Some(a), Some(b)) = (&first.1.head, &second.1.head) else {
            let files = shared(first.1, second.1, None);
            self.push(first.0, second.0, files);
            return;
        };
        let key = (a.clone(), b.clone());
        let Some(parted) = known.get(&key).cloned().or_else(|| parted(first.0, a, b).ok().map(Arc::new)) else {
            return;
        };
        let files = shared(first.1, second.1, Some(&parted));
        self.known.insert(key, parted);
        self.push(first.0, second.0, files);
    }

    fn push(&mut self, first: &Path, second: &Path, files: Vec<String>) {
        if !files.is_empty() {
            self.pairs.push(((first.to_path_buf(), second.to_path_buf()), files));
        }
    }
}

pub(crate) fn shared(a: &Work, b: &Work, parted: Option<&Parted>) -> Vec<String> {
    let first = changed(a, parted.map(|p| &p.first));
    let second = changed(b, parted.map(|p| &p.second));
    first.intersection(&second).map(|path| (*path).clone()).collect()
}

fn changed<'a>(work: &'a Work, parted: Option<&'a BTreeSet<String>>) -> BTreeSet<&'a String> {
    work.files.iter().chain(parted.into_iter().flatten()).collect()
}

fn work_of(dir: &Path) -> Result<Work> {
    let head = head_of(dir)?;
    let from = git::head(dir)?;
    let mut files =
        names(&git::run(dir, &["diff", "--name-only", "-z", "--no-renames", "--relative", &from, "--", "."])?);
    files.extend(names(&git::run(dir, &["ls-files", "--others", "--exclude-standard", "-z"])?));
    Ok(Work { head, files })
}

fn head_of(dir: &Path) -> Result<Option<String>> {
    let out = git::run_raw(dir, &["rev-parse", "--verify", "--quiet", "HEAD"], &[])?;
    Ok(out.status.success().then(|| String::from_utf8_lossy(&out.stdout).trim().to_string()))
}

fn parted(dir: &Path, first: &str, second: &str) -> Result<Parted> {
    let out = git::run_raw(dir, &["merge-base", first, second], &[])?;
    if out.status.code() == Some(1) {
        return Ok(Parted::default());
    }
    let base = String::from_utf8_lossy(&crate::git::worktree::check(out)?).trim().to_string();
    Ok(Parted { first: since(dir, &base, first)?, second: since(dir, &base, second)? })
}

fn since(dir: &Path, base: &str, head: &str) -> Result<BTreeSet<String>> {
    let args = ["diff", "--name-only", "-z", "--no-renames", "--relative", base, head, "--", "."];
    git::run(dir, &args).map(|out| names(&out))
}

fn names(out: &[u8]) -> BTreeSet<String> {
    out.split(|b| *b == 0).filter(|p| !p.is_empty()).map(|p| String::from_utf8_lossy(p).into_owned()).collect()
}

#[cfg(test)]
mod tests;
