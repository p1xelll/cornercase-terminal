use std::os::unix::fs::PermissionsExt;

use super::*;
use crate::plugins::registry::{Entry, Source};
use crate::test_util::{TempDir, wait_until, write_executable};

fn plugin(root: &Path) -> Plugin {
    let entry = Entry { id: "acme.notes".into(), source: Source::Link { path: root.to_path_buf() }, enabled: true };
    Plugin { entry, manifest: Err("not read in these tests".into()) }
}

fn mode(path: &Path) -> u32 {
    std::fs::metadata(path).expect("exists").permissions().mode() & 0o777
}

#[test]
fn a_command_gets_its_folders_and_none_of_the_callers_identity() {
    let (home, root) = (TempDir::new(), TempDir::new());
    let places = Places::new(&home.path().join("config.json"), &home.path().join("session.json"));
    let script = "#!/bin/sh\necho \"id=$CORNERCASE_PLUGIN_ID root=$CORNERCASE_PLUGIN_ROOT config=$CORNERCASE_PLUGIN_CONFIG \
                  pane=${CORNERCASE_PANE-unset} server=${CORNERCASE_SERVER-unset} extra=$EXTRA\"\n";
    write_executable(&root.path().join("run"), script);
    let plugin = plugin(root.path());
    let mut command =
        places.command(&plugin, &["./run".into()], &[("EXTRA".into(), "yes".into())], None).expect("command");
    let status = command.status().expect("ran");
    let log = std::fs::read_to_string(places.log("acme.notes")).expect("log");
    let config = places.config("acme.notes");
    let expected = format!(
        "id=acme.notes root={} config={} pane=unset server=unset extra=yes\n",
        root.path().display(),
        config.display()
    );
    assert_eq!((status.success(), log), (true, expected));
    assert_eq!(
        (mode(&config), mode(&places.state("acme.notes")), mode(&places.log("acme.notes"))),
        (0o700, 0o700, 0o600)
    );
}

#[test]
fn a_command_that_outlives_its_time_is_stopped_with_its_children() {
    let (home, root) = (TempDir::new(), TempDir::new());
    let places = Places::new(&home.path().join("config.json"), &home.path().join("session.json"));
    write_executable(&root.path().join("hang"), "#!/bin/sh\nsleep 30 &\necho $! > child\nwait\n");
    let child =
        places.command(&plugin(root.path()), &["./hang".into()], &[], None).expect("command").spawn().expect("spawn");
    let pid_file = root.path().join("child");
    wait_until("the child starts", || std::fs::read_to_string(&pid_file).is_ok_and(|s| s.ends_with('\n')));
    let grandchild: i32 = std::fs::read_to_string(&pid_file).expect("pid").trim().parse().expect("a pid");
    assert_eq!(finish(child, Duration::from_millis(200)).expect("waited"), None);
    wait_until("the sleep is gone", || !crate::process::alive(grandchild));
}
