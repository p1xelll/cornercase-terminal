use std::path::{Path, PathBuf};

use rstest::rstest;

use super::*;
use crate::test_util::{Checkouts, TempDir, git};

fn write(dir: &Path, name: &str, text: &str) {
    std::fs::write(dir.join(name), text).expect("write file");
}

fn commit(dir: &Path, message: &str) {
    git(dir, &["add", "--all"]);
    git(dir, &["commit", "--quiet", "-m", message]);
}

fn files(pairs: &[Pair]) -> Vec<Vec<&str>> {
    pairs.iter().map(|(_, files)| files.iter().map(String::as_str).collect()).collect()
}

mod a_workspace_s_work {
    use super::*;

    #[test]
    fn is_its_commits_its_edits_and_its_new_files() {
        let mut project = Checkouts::new(&[("a", "1\n"), ("b", "1\n"), ("d", "1\n")]);
        let (first, second) = (project.add("w1", "main"), project.add("w2", "main"));
        write(&first, "a", "first\n");
        commit(&first, "a");
        write(&first, "b", "first\n");
        write(&first, "new", "first\n");
        write(&second, "a", "second\n");
        commit(&second, "a");
        write(&second, "b", "second\n");
        write(&second, "new", "second\n");
        write(project.repo.path(), "d", "main\n");
        commit(project.repo.path(), "d");

        let scan = scan(&[first, second], &Known::new());

        assert_eq!(files(&scan.pairs), [vec!["a", "b", "new"]]);
    }

    #[test]
    fn counts_both_paths_of_a_rename() {
        let mut project = Checkouts::new(&[("x", "one\ntwo\nthree\n")]);
        let (first, second) = (project.add("w1", "main"), project.add("w2", "main"));
        git(&first, &["mv", "x", "y"]);
        write(&second, "x", "one\ntwo\nthree\nfour\n");

        let scan = scan(&[first, second], &Known::new());

        assert_eq!(files(&scan.pairs), [vec!["x"]]);
    }

    #[test]
    fn leaves_out_commits_a_branch_inherited() {
        let mut project = Checkouts::new(&[("a", "1\n"), ("b", "1\n")]);
        let first = project.add("w1", "main");
        write(&first, "a", "first\n");
        commit(&first, "a");
        let second = project.add("w2", "w1");
        write(&second, "b", "second\n");
        assert_eq!(scan(&[first.clone(), second.clone()], &Known::new()).pairs, Vec::<Pair>::new());

        write(&first, "b", "first\n");
        let scan = scan(&[first, second], &Known::new());

        assert_eq!(files(&scan.pairs), [vec!["b"]]);
    }
}

mod the_pairs {
    use super::*;

    #[test]
    fn reuse_what_they_know() {
        let mut project = Checkouts::new(&[("a", "1\n")]);
        let (first, second) = (project.add("w1", "main"), project.add("w2", "main"));
        let dirs = [first, second];
        let known = scan(&dirs, &Known::new()).known;
        assert_eq!(known.len(), 1);
        let key = known.keys().next().expect("one pair").clone();

        let fake = Parted { first: ["fake".to_string()].into(), second: ["fake".to_string()].into() };
        let swapped = Known::from([(key, Arc::new(fake))]);
        let again = scan(&dirs, &swapped);

        assert_eq!(files(&again.pairs), [vec!["fake"]]);
    }

    #[test]
    fn never_pair_a_folder_with_itself() {
        let mut project = Checkouts::new(&[("a", "1\n")]);
        let only = project.add("w1", "main");
        write(&only, "a", "edited\n");

        assert_eq!(scan(&[only.clone(), only], &Known::new()).pairs, Vec::<Pair>::new());
    }

    #[test]
    fn leave_out_a_folder_that_is_not_a_repository() {
        let mut project = Checkouts::new(&[("a", "1\n")]);
        let (first, second) = (project.add("w1", "main"), project.add("w2", "main"));
        write(&first, "a", "first\n");
        write(&second, "a", "second\n");
        let plain = TempDir::new();

        let scan = scan(&[first.clone(), plain.path().to_path_buf(), second.clone()], &Known::new());

        let paired: Vec<&(PathBuf, PathBuf)> = scan.pairs.iter().map(|(pair, _)| pair).collect();
        assert_eq!(paired, [&(first, second)]);
    }
}

fn work(files: &[&str]) -> Work {
    Work { head: None, files: files.iter().map(ToString::to_string).collect() }
}

#[rstest]
#[case::disjoint(&["a"], &["b"], vec![])]
#[case::one_in_common(&["a", "b"], &["b", "c"], vec!["b"])]
#[case::sorted(&["z", "a", "m"], &["m", "z", "a"], vec!["a", "m", "z"])]
fn shared_files(#[case] first: &[&str], #[case] second: &[&str], #[case] expected: Vec<&str>) {
    assert_eq!(shared(&work(first), &work(second), None), expected);
}

#[test]
fn shared_adds_what_each_side_committed_since_they_parted() {
    let parted = Parted { first: ["a".to_string()].into(), second: ["b".to_string()].into() };

    assert_eq!(shared(&work(&["b"]), &work(&["a"]), Some(&parted)), ["a", "b"]);
}
