use std::io;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use serde::{Deserialize, Serialize};

use super::manifest::{self, Manifest};
use crate::persist;

const VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Entry {
    pub(crate) id: String,
    pub(crate) source: Source,
    #[serde(default)]
    pub(crate) enabled: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub(crate) enum Source {
    Link { path: PathBuf },
}

impl Source {
    pub(crate) fn root(&self) -> &Path {
        match self {
            Self::Link { path } => path,
        }
    }

    pub(crate) fn describe(&self) -> String {
        match self {
            Self::Link { path } => format!("linked from {}", path.display()),
        }
    }
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct Saved {
    version: u32,
    #[serde(default)]
    plugins: Vec<Entry>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Plugin {
    pub(crate) entry: Entry,
    pub(crate) manifest: Result<Manifest, String>,
}

impl Plugin {
    pub(crate) fn runs(&self) -> Option<&Manifest> {
        self.manifest.as_ref().ok().filter(|_| self.entry.enabled)
    }

    pub(crate) fn root(&self) -> &Path {
        self.entry.source.root()
    }

    pub(crate) fn name(&self) -> &str {
        self.manifest.as_ref().map_or(&self.entry.id, |m| &m.name)
    }
}

#[derive(Debug)]
pub(crate) struct Registry {
    path: PathBuf,
    pub(crate) entries: Vec<Entry>,
}

impl Registry {
    pub(crate) fn load(path: &Path) -> Self {
        let entries = std::fs::read(path)
            .ok()
            .and_then(|bytes| serde_json::from_slice::<Saved>(&bytes).ok())
            .filter(|saved| saved.version == VERSION)
            .map(|saved| saved.plugins)
            .unwrap_or_default();
        Self { path: path.to_path_buf(), entries }
    }

    pub(crate) fn save(&self) -> io::Result<()> {
        persist::write(&self.path, &Saved { version: VERSION, plugins: self.entries.clone() }, true)
    }

    pub(crate) fn plugins(&self) -> Vec<Plugin> {
        self.entries
            .iter()
            .map(|entry| Plugin { entry: entry.clone(), manifest: Manifest::load(entry.source.root()) })
            .collect()
    }

    pub(crate) fn link(&mut self, folder: &Path) -> Result<Manifest, String> {
        let path = folder.canonicalize().map_err(|e| format!("cannot open {}: {e}", folder.display()))?;
        let manifest = Manifest::load(&path)?;
        let source = Source::Link { path };
        match self.entries.iter_mut().find(|entry| entry.id == manifest.id) {
            Some(entry) if entry.source == source => {}
            Some(entry) => {
                return Err(format!("a plugin called `{}` is already {}", manifest.id, entry.source.describe()));
            }
            None => self.entries.push(Entry { id: manifest.id.clone(), source, enabled: false }),
        }
        Ok(manifest)
    }

    pub(crate) fn unlink(&mut self, id: &str) -> bool {
        let before = self.entries.len();
        self.entries.retain(|entry| entry.id != id);
        self.entries.len() != before
    }

    pub(crate) fn set_enabled(&mut self, id: &str, on: bool) -> Result<Plugin, String> {
        let entry = self.entries.iter_mut().find(|entry| entry.id == id).ok_or_else(|| missing(id))?;
        let manifest = Manifest::load(entry.source.root());
        if on && let Err(e) = &manifest {
            return Err(format!("`{id}` cannot run: {e}"));
        }
        entry.enabled = on;
        Ok(Plugin { entry: entry.clone(), manifest })
    }
}

pub(crate) fn missing(id: &str) -> String {
    format!("no plugin called `{id}`; `cornercase plugin list` shows them")
}

type Mark = Option<(u64, SystemTime, u64)>;

fn mark(path: &Path) -> Mark {
    let meta = std::fs::metadata(path).ok()?;
    Some((meta.ino(), meta.modified().ok()?, meta.len()))
}

#[derive(Debug)]
pub(crate) struct Watch {
    path: PathBuf,
    marks: Option<Vec<Mark>>,
}

impl Watch {
    pub(crate) fn new(path: PathBuf) -> Self {
        Self { path, marks: None }
    }

    pub(crate) fn changed(&mut self) -> Option<Vec<Plugin>> {
        let registry = Registry::load(&self.path);
        let mut marks = vec![mark(&self.path)];
        marks.extend(registry.entries.iter().map(|entry| mark(&entry.source.root().join(manifest::FILE))));
        if self.marks.as_ref() == Some(&marks) {
            return None;
        }
        self.marks = Some(marks);
        Some(registry.plugins())
    }
}

#[cfg(test)]
mod tests;
