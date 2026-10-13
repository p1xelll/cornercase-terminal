use std::fs::{File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::Path;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::thread;

use rustix::fs::{FlockOperation, flock};

use super::input;
use crate::error::{Error, Result};
use crate::host::notch::{Action, Board};
use crate::process;

#[derive(Default)]
pub(crate) struct Runner {
    running: Option<Running>,
}

struct Running {
    child: Child,
    input: ChildStdin,
    _lock: File,
}

impl Runner {
    pub(crate) fn show(&mut self, board: &Board, lock: &Path, actions: impl Fn(Action) -> bool + Send + 'static) {
        if self.running.is_none() {
            match start(lock, actions) {
                Ok(running) => self.running = running,
                Err(e) => input::report(&format!("the notch could not start: {e}")),
            }
        }
        let Some(running) = &mut self.running else { return };
        let sent = serde_json::to_string(board)
            .map_err(std::io::Error::other)
            .and_then(|line| writeln!(running.input, "{line}").and_then(|()| running.input.flush()));
        if sent.is_err() {
            self.stop();
        }
    }

    pub(crate) fn stop(&mut self) {
        if let Some(running) = self.running.take() {
            process::stop(running.child);
        }
    }

    pub(crate) fn running(&self) -> bool {
        self.running.is_some()
    }
}

impl Drop for Runner {
    fn drop(&mut self) {
        self.stop();
    }
}

fn start(lock: &Path, actions: impl Fn(Action) -> bool + Send + 'static) -> Result<Option<Running>> {
    let file = OpenOptions::new().create(true).write(true).truncate(false).open(lock)?;
    if flock(&file, FlockOperation::NonBlockingLockExclusive).is_err() {
        return Ok(None);
    }
    let mut child = Command::new(std::env::current_exe()?)
        .arg("notch")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()?;
    let (Some(input), Some(output)) = (child.stdin.take(), child.stdout.take()) else {
        process::stop(child);
        return Err(Error::Control("the notch started without its pipes".into()));
    };
    thread::spawn(move || {
        for line in BufReader::new(output).lines() {
            let Ok(line) = line else { return };
            if let Ok(action) = serde_json::from_str(&line)
                && !actions(action)
            {
                return;
            }
        }
    });
    Ok(Some(Running { child, input, _lock: file }))
}
