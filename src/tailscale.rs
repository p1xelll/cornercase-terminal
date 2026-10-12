use std::fmt;
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use serde_json::Value;

pub const HTTPS_PORT: u16 = 8443;
pub const ADMIN_DNS: &str = "https://login.tailscale.com/admin/dns";
const APP_BINARY: &str = "/Applications/Tailscale.app/Contents/MacOS/Tailscale";
const TIMEOUT: Duration = Duration::from_secs(15);
const POLL: Duration = Duration::from_millis(20);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Node {
    pub host: String,
    pub login: String,
}

impl Node {
    pub fn url(&self) -> String {
        format!("https://{}:{HTTPS_PORT}", self.host)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Problem {
    NotInstalled,
    NotConnected(String),
    NoHttps,
    Failed(String),
}

impl fmt::Display for Problem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotInstalled => write!(f, "tailscale is not installed on this computer"),
            Self::NotConnected(state) => write!(f, "tailscale is not connected ({state}): open it and log in"),
            Self::NoHttps => write!(f, "turn on HTTPS certificates for your tailnet at {ADMIN_DNS}"),
            Self::Failed(why) => write!(f, "tailscale: {why}"),
        }
    }
}

pub fn binary() -> Option<PathBuf> {
    let on_path = std::env::var_os("PATH")
        .into_iter()
        .flat_map(|path| std::env::split_paths(&path).collect::<Vec<_>>())
        .map(|dir| dir.join("tailscale"))
        .find(|candidate| candidate.is_file());
    on_path.or_else(|| Some(PathBuf::from(APP_BINARY)).filter(|app| app.is_file()))
}

pub fn start(program: &Path, local_port: u16) -> Result<Node, Problem> {
    let node = status(program)?;
    let target = format!("http://127.0.0.1:{local_port}");
    let https = format!("--https={HTTPS_PORT}");
    run(program, &["serve", "--bg", "--yes", &https, &target])?;
    Ok(node)
}

pub fn stop(program: &Path) -> Result<(), Problem> {
    let https = format!("--https={HTTPS_PORT}");
    run(program, &["serve", &https, "off"]).map(|_| ())
}

pub fn status(program: &Path) -> Result<Node, Problem> {
    let output = run(program, &["status", "--json"])?;
    parse_status(&String::from_utf8_lossy(&output.stdout))
}

pub fn parse_status(text: &str) -> Result<Node, Problem> {
    let status: Value =
        serde_json::from_str(text).map_err(|e| Problem::Failed(format!("unexpected status output: {e}")))?;
    let state = status["BackendState"].as_str().unwrap_or("unknown");
    if state != "Running" {
        return Err(Problem::NotConnected(state.into()));
    }
    let host = status["Self"]["DNSName"].as_str().unwrap_or_default().trim_end_matches('.').to_string();
    if host.is_empty() {
        return Err(Problem::Failed("this computer has no tailnet name; turn on MagicDNS".into()));
    }
    let certified =
        status["CertDomains"].as_array().is_some_and(|domains| domains.iter().any(|d| d.as_str() == Some(&host)));
    if !certified {
        return Err(Problem::NoHttps);
    }
    let user = status["Self"]["UserID"].as_u64().map(|id| id.to_string()).unwrap_or_default();
    let login = status["User"][&user]["LoginName"].as_str().unwrap_or_default().to_string();
    if login.is_empty() {
        return Err(Problem::Failed("cannot tell which account owns this computer".into()));
    }
    Ok(Node { host, login })
}

fn drain(pipe: Option<impl Read + Send + 'static>) -> thread::JoinHandle<Vec<u8>> {
    thread::spawn(move || {
        let mut bytes = Vec::new();
        if let Some(mut pipe) = pipe {
            let _ = pipe.read_to_end(&mut bytes);
        }
        bytes
    })
}

fn wait(child: &mut Child, timeout: Duration) -> io::Result<Option<std::process::ExitStatus>> {
    let deadline = Instant::now() + timeout;
    loop {
        if let Some(status) = child.try_wait()? {
            return Ok(Some(status));
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            return Ok(None);
        }
        thread::sleep(POLL);
    }
}

fn run(program: &Path, args: &[&str]) -> Result<Output, Problem> {
    let spawned =
        Command::new(program).args(args).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn();
    let mut child = match spawned {
        Ok(child) => child,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Err(Problem::NotInstalled),
        Err(e) => return Err(Problem::Failed(e.to_string())),
    };
    let (stdout, stderr) = (drain(child.stdout.take()), drain(child.stderr.take()));
    let status = wait(&mut child, TIMEOUT).map_err(|e| Problem::Failed(e.to_string()))?;
    let output = Output {
        status: status.ok_or_else(|| Problem::Failed(format!("no answer within {} s", TIMEOUT.as_secs())))?,
        stdout: stdout.join().unwrap_or_default(),
        stderr: stderr.join().unwrap_or_default(),
    };
    if output.status.success() {
        return Ok(output);
    }
    let said = String::from_utf8_lossy(&output.stderr);
    let last = said.lines().map(str::trim).rfind(|line| !line.is_empty()).map(String::from);
    Err(Problem::Failed(last.unwrap_or_else(|| output.status.to_string())))
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;
    use crate::test_util::{TempDir, write_executable};

    fn status_json(state: &str, certs: &str) -> String {
        format!(
            r#"{{
  "BackendState": "{state}",
  "CertDomains": {certs},
  "Self": {{"DNSName": "mac.tail1234.ts.net.", "UserID": 42}},
  "User": {{"42": {{"ID": 42, "LoginName": "ada@example.com"}}}}
}}"#
        )
    }

    mod parse_status {
        use super::*;

        #[test]
        fn a_running_node_with_https_gives_its_name_and_owner() {
            let node = parse_status(&status_json("Running", r#"["mac.tail1234.ts.net"]"#)).expect("a node");

            assert_eq!(node, Node { host: "mac.tail1234.ts.net".into(), login: "ada@example.com".into() });
            assert_eq!(node.url(), "https://mac.tail1234.ts.net:8443");
        }

        #[rstest]
        #[case::logged_out("NeedsLogin", r#"["mac.tail1234.ts.net"]"#, Problem::NotConnected("NeedsLogin".into()))]
        #[case::stopped("Stopped", "null", Problem::NotConnected("Stopped".into()))]
        #[case::no_certificates("Running", "null", Problem::NoHttps)]
        #[case::certificates_for_another_name("Running", r#"["other.tail1234.ts.net"]"#, Problem::NoHttps)]
        fn says_what_is_missing(#[case] state: &str, #[case] certs: &str, #[case] problem: Problem) {
            assert_eq!(parse_status(&status_json(state, certs)), Err(problem));
        }

        #[test]
        fn output_that_is_not_json_is_a_failure() {
            assert!(matches!(parse_status("Tailscale is stopped."), Err(Problem::Failed(_))));
        }
    }

    mod commands {
        use super::*;

        fn fake(dir: &TempDir, script: &str) -> PathBuf {
            let path = dir.path().join("tailscale");
            write_executable(&path, &format!("#!/bin/sh\n{script}\n"));
            path
        }

        #[test]
        fn start_serves_the_local_port_on_the_https_port() {
            let dir = TempDir::new();
            let log = dir.path().join("args");
            let status = status_json("Running", r#"["mac.tail1234.ts.net"]"#);
            let program = fake(
                &dir,
                &format!(
                    "echo \"$@\" >> '{}'\nif [ \"$1\" = status ]; then cat <<'EOF'\n{status}\nEOF\nfi",
                    log.display()
                ),
            );

            let node = start(&program, 4321).expect("started");

            assert_eq!(node.host, "mac.tail1234.ts.net");
            let calls = std::fs::read_to_string(&log).expect("calls");
            assert_eq!(calls, "status --json\nserve --bg --yes --https=8443 http://127.0.0.1:4321\n");
        }

        #[test]
        fn stop_turns_the_https_port_off() {
            let dir = TempDir::new();
            let log = dir.path().join("args");
            let program = fake(&dir, &format!("echo \"$@\" >> '{}'", log.display()));

            stop(&program).expect("stopped");

            assert_eq!(std::fs::read_to_string(&log).expect("calls"), "serve --https=8443 off\n");
        }

        #[test]
        fn a_failing_command_reports_its_last_line() {
            let dir = TempDir::new();
            let program = fake(&dir, "echo 'first' >&2\necho 'serve config denied' >&2\nexit 1");

            assert_eq!(stop(&program), Err(Problem::Failed("serve config denied".into())));
        }

        #[test]
        fn a_command_that_hangs_is_killed() {
            let dir = TempDir::new();
            let program = fake(&dir, "exec sleep 30");
            let mut child = Command::new(&program).stdin(Stdio::null()).spawn().expect("started");

            let status = wait(&mut child, Duration::from_millis(300)).expect("waited");

            assert_eq!(status, None);
            assert!(child.try_wait().expect("reaped").is_some());
        }

        #[test]
        fn a_missing_program_is_not_installed() {
            assert_eq!(status(Path::new("/nonexistent/tailscale")), Err(Problem::NotInstalled));
        }
    }
}
