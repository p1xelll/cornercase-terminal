use std::fmt::Write as _;
use std::io::{self, Write as _};

use serde_json::json;

use super::args::PluginAction;
use super::print_log;
use crate::error::{Error, Result};
use crate::paths;
use crate::plugins::registry::missing;
use crate::plugins::{Places, Plugin, Registry};

pub(super) fn run(action: PluginAction) -> Result<bool> {
    let places = Places::new(&paths::config(), &paths::session());
    let mut registry = Registry::load(&places.registry);
    match action {
        PluginAction::Link { folder } => {
            let manifest = registry.link(&folder).map_err(Error::Plugin)?;
            registry.save()?;
            println!("linked {} {} ({}), turned off", manifest.name, manifest.version, manifest.id);
            println!("turn it on with `cornercase plugin enable {}`", manifest.id);
        }
        PluginAction::Unlink { id } => {
            if !registry.unlink(&id) {
                return Err(Error::Plugin(missing(&id)));
            }
            registry.save()?;
            println!("unlinked {id}; its folder and its files stay where they are");
        }
        PluginAction::List(print) => {
            let plugins = registry.plugins();
            let mut out = io::stdout().lock();
            let text = if print.json { list_json(&plugins, &places) } else { list_text(&plugins) };
            if super::quietly(out.write_all(text.as_bytes()).and_then(|()| out.flush()))? {
                return Ok(true);
            }
        }
        PluginAction::Enable { id } => {
            let plugin = registry.set_enabled(&id, true).map_err(Error::Plugin)?;
            registry.save()?;
            let Ok(manifest) = &plugin.manifest else { return Ok(true) };
            println!("{} is on. It runs as you and can do anything you can, in every terminal.", manifest.name);
            println!("It {}; that is shown, not enforced.", manifest.asks());
            if !manifest.runs().is_empty() {
                println!("It runs {}.", manifest.runs().join(", "));
            }
        }
        PluginAction::Disable { id } => {
            let plugin = registry.set_enabled(&id, false).map_err(Error::Plugin)?;
            registry.save()?;
            println!("{} is off", plugin.name());
        }
        PluginAction::Logs { id, logs } => {
            if !registry.entries.iter().any(|entry| entry.id == id) {
                return Err(Error::Plugin(missing(&id)));
            }
            return print_log(&places.log(&id), &logs);
        }
    }
    Ok(true)
}

fn list_text(plugins: &[Plugin]) -> String {
    if plugins.is_empty() {
        return "no plugins; `cornercase plugin link <folder>` adds one\n".into();
    }
    let mut text = String::new();
    for plugin in plugins {
        let on = if plugin.entry.enabled { "on" } else { "off" };
        match &plugin.manifest {
            Ok(manifest) => {
                let _ = writeln!(text, "{}  {}  {on}  {}", plugin.entry.id, manifest.version, manifest.name);
                let _ = writeln!(text, "  {}\n  {}", plugin.entry.source.describe(), manifest.asks());
                if !manifest.runs().is_empty() {
                    let _ = writeln!(text, "  runs {}", manifest.runs().join(", "));
                }
            }
            Err(e) => {
                let _ = writeln!(text, "{}  -  {on}  cannot run", plugin.entry.id);
                let _ = writeln!(text, "  {}\n  {e}", plugin.entry.source.describe());
            }
        }
    }
    text
}

fn list_json(plugins: &[Plugin], places: &Places) -> String {
    let rows: Vec<_> = plugins
        .iter()
        .map(|plugin| {
            let (manifest, error) = match &plugin.manifest {
                Ok(manifest) => (serde_json::to_value(manifest).ok(), None),
                Err(e) => (None, Some(e.clone())),
            };
            json!({
                "id": plugin.entry.id,
                "enabled": plugin.entry.enabled,
                "source": plugin.entry.source,
                "log": places.log(&plugin.entry.id),
                "manifest": manifest,
                "error": error,
            })
        })
        .collect();
    let mut text = serde_json::to_string_pretty(&json!({ "plugins": rows })).unwrap_or_default();
    text.push('\n');
    text
}
