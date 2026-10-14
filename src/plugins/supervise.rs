use std::collections::BTreeMap;
use std::process::Child;
use std::sync::Arc;
use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use parking_lot::Mutex;
use rustix::process::Signal;

use super::registry::{Plugin, Watch};
use super::run::Places;
use crate::log;
use crate::panics;

const CHECK_EVERY: Duration = Duration::from_millis(250);
const HEALTHY_AFTER: Duration = Duration::from_secs(60);
const MAX_BACKOFF: Duration = Duration::from_secs(60);
const STOP_WITHIN: Duration = Duration::from_secs(2);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Kind {
    Service,
    Window,
}

impl Kind {
    fn name(self) -> &'static str {
        match self {
            Self::Service => "service",
            Self::Window => "window process",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct Key {
    plugin: String,
    entry: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Spec {
    plugin: Plugin,
    command: Vec<String>,
}

struct Running {
    spec: Spec,
    child: Child,
    since: Instant,
}

struct Waiting {
    failures: u32,
    at: Instant,
}

pub(crate) type Notes = Arc<Mutex<BTreeMap<String, String>>>;

pub(crate) struct Supervisor {
    stop: Option<Sender<()>>,
    thread: Option<JoinHandle<()>>,
    notes: Notes,
}

impl Supervisor {
    pub(crate) fn start(kind: Kind, places: Places, env: Vec<(String, String)>) -> Self {
        let (tx, rx) = mpsc::channel();
        let notes = Notes::default();
        let shared = notes.clone();
        let thread = thread::spawn(move || {
            let mut state =
                State { kind, places, env, notes: shared, running: BTreeMap::new(), waiting: BTreeMap::new() };
            let mut watch = Watch::new(state.places.registry.clone());
            let mut wanted = BTreeMap::new();
            loop {
                if let Some(plugins) = watch.changed() {
                    wanted = state.wanted(&plugins);
                }
                let _ = panics::contain(|| state.step(&wanted, Instant::now()));
                match rx.recv_timeout(CHECK_EVERY) {
                    Err(RecvTimeoutError::Timeout) => {}
                    Ok(()) | Err(RecvTimeoutError::Disconnected) => break,
                }
            }
            state.stop_all();
        });
        Self { stop: Some(tx), thread: Some(thread), notes }
    }

    pub(crate) fn note(&self, plugin: &str) -> Option<String> {
        self.notes.lock().get(plugin).cloned()
    }
}

impl Drop for Supervisor {
    fn drop(&mut self) {
        drop(self.stop.take());
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

struct State {
    kind: Kind,
    places: Places,
    env: Vec<(String, String)>,
    notes: Notes,
    running: BTreeMap<Key, Running>,
    waiting: BTreeMap<Key, Waiting>,
}

impl State {
    fn wanted(&self, plugins: &[Plugin]) -> BTreeMap<Key, Spec> {
        let mut wanted = BTreeMap::new();
        for plugin in plugins {
            let Some(manifest) = plugin.runs() else { continue };
            let entries = match self.kind {
                Kind::Service => &manifest.service,
                Kind::Window => &manifest.window,
            };
            for entry in entries {
                let key = Key { plugin: plugin.entry.id.clone(), entry: entry.id.clone() };
                wanted.insert(key, Spec { plugin: plugin.clone(), command: entry.command.clone() });
            }
        }
        wanted
    }

    fn step(&mut self, wanted: &BTreeMap<Key, Spec>, now: Instant) {
        let gone: Vec<Key> = self
            .running
            .iter()
            .filter(|(key, run)| wanted.get(key) != Some(&run.spec))
            .map(|(k, _)| k.clone())
            .collect();
        for key in gone {
            if let Some(run) = self.running.remove(&key) {
                self.stop(&key, run);
            }
        }
        self.waiting.retain(|key, _| wanted.contains_key(key));
        self.reap(now);
        for (key, spec) in wanted {
            if self.running.contains_key(key) || self.waiting.get(key).is_some_and(|w| w.at > now) {
                continue;
            }
            self.spawn(key, spec, now);
        }
        self.publish(now);
    }

    fn spawn(&mut self, key: &Key, spec: &Spec, now: Instant) {
        let kind = self.kind.name();
        let child = self.places.command(&spec.plugin, &spec.command, &self.env, None).and_then(|mut c| c.spawn());
        match child {
            Ok(child) => {
                log::info!("plugins", "started", kind = kind, plugin = key.plugin, entry = key.entry, pid = child.id());
                self.places.note(
                    &key.plugin,
                    &format!("{kind} started"),
                    &[("entry", &key.entry), ("pid", &child.id())],
                );
                self.running.insert(key.clone(), Running { spec: spec.clone(), child, since: now });
            }
            Err(e) => {
                log::warning!(
                    "plugins",
                    "cannot start",
                    kind = kind,
                    plugin = key.plugin,
                    entry = key.entry,
                    error = e
                );
                self.places.note(&key.plugin, &format!("{kind} cannot start"), &[("entry", &key.entry), ("error", &e)]);
                self.retry(key, now, false);
            }
        }
    }

    fn reap(&mut self, now: Instant) {
        let mut ended = Vec::new();
        for (key, run) in &mut self.running {
            match run.child.try_wait() {
                Ok(Some(status)) => ended.push((key.clone(), status.to_string(), now.duration_since(run.since))),
                Ok(None) => {}
                Err(e) => ended.push((key.clone(), e.to_string(), now.duration_since(run.since))),
            }
        }
        for (key, status, ran) in ended {
            self.running.remove(&key);
            let kind = self.kind.name();
            log::warning!("plugins", "exited", kind = kind, plugin = key.plugin, entry = key.entry, status = status);
            self.places.note(&key.plugin, &format!("{kind} exited"), &[("entry", &key.entry), ("status", &status)]);
            self.retry(&key, now, ran >= HEALTHY_AFTER);
        }
    }

    fn retry(&mut self, key: &Key, now: Instant, healthy: bool) {
        let failures = if healthy { 1 } else { self.waiting.get(key).map_or(1, |w| w.failures + 1) };
        let wait = backoff(failures);
        self.waiting.insert(key.clone(), Waiting { failures, at: now + wait });
    }

    fn publish(&self, now: Instant) {
        let mut notes = BTreeMap::new();
        for key in self.running.keys() {
            notes.entry(key.plugin.clone()).or_insert_with(|| format!("{} {} runs", self.kind.name(), key.entry));
        }
        for (key, waiting) in &self.waiting {
            if self.running.contains_key(key) {
                continue;
            }
            let secs = waiting.at.saturating_duration_since(now).as_secs() + 1;
            let note = format!("{} {} stopped · starting again in {secs} s", self.kind.name(), key.entry);
            notes.insert(key.plugin.clone(), note);
        }
        *self.notes.lock() = notes;
    }

    fn stop(&self, key: &Key, run: Running) {
        stop_children(vec![run]);
        let kind = self.kind.name();
        log::info!("plugins", "stopped", kind = kind, plugin = key.plugin, entry = key.entry);
        self.places.note(&key.plugin, &format!("{kind} stopped"), &[("entry", &key.entry)]);
    }

    fn stop_all(&mut self) {
        let running = std::mem::take(&mut self.running);
        for key in running.keys() {
            self.places.note(&key.plugin, &format!("{} stopped", self.kind.name()), &[("entry", &key.entry)]);
        }
        if !running.is_empty() {
            log::info!("plugins", "stopped every one", kind = self.kind.name(), count = running.len());
        }
        stop_children(running.into_values().collect());
    }
}

pub(crate) fn backoff(failures: u32) -> Duration {
    Duration::from_secs(1u64 << failures.saturating_sub(1).min(6)).min(MAX_BACKOFF)
}

fn stop_children(mut running: Vec<Running>) {
    for run in &running {
        super::run::signal(&run.child, Signal::TERM);
    }
    let until = Instant::now() + STOP_WITHIN;
    while Instant::now() < until {
        running.retain_mut(|run| !matches!(run.child.try_wait(), Ok(Some(_)) | Err(_)));
        if running.is_empty() {
            return;
        }
        thread::sleep(Duration::from_millis(20));
    }
    for mut run in running {
        super::run::signal(&run.child, Signal::KILL);
        let _ = run.child.wait();
    }
}

#[cfg(test)]
mod tests;
