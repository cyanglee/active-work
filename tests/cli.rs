use std::collections::HashSet;
use std::path::Path;
use std::process::{Command, Output, Stdio};

use tempfile::tempdir;

fn aw(home: &Path, args: &[&str]) -> Output {
    aw_in(home, Path::new(env!("CARGO_MANIFEST_DIR")), args)
}

fn aw_in(home: &Path, current_dir: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_aw"))
        .env("AW_HOME", home)
        .current_dir(current_dir)
        .args(args)
        .output()
        .unwrap()
}

fn stdout(output: &Output) -> String {
    String::from_utf8(output.stdout.clone()).unwrap()
}

fn stderr(output: &Output) -> String {
    String::from_utf8(output.stderr.clone()).unwrap()
}

#[test]
fn start_update_list_and_finish_a_task() {
    let home = tempdir().unwrap();

    let started = aw(
        home.path(),
        &[
            "start",
            "CB-142",
            "--project",
            "ClinicBase",
            "--title",
            "Fix invoice rounding",
            "--summary",
            "Reproduced the bug",
            "--next",
            "Inspect the calculator",
        ],
    );
    assert!(started.status.success(), "{}", stderr(&started));

    let updated = aw(
        home.path(),
        &[
            "update",
            "CB-142",
            "--summary",
            "Found the rounding point",
            "--next",
            "Run invoice specs",
        ],
    );
    assert!(updated.status.success(), "{}", stderr(&updated));

    let listed = aw(home.path(), &[]);
    assert!(listed.status.success(), "{}", stderr(&listed));
    let listing = stdout(&listed);
    assert!(listing.contains("CB-142"));
    assert!(listing.contains("Found the rounding point"));
    assert!(listing.contains("→ Run invoice specs"));

    let completed = aw(
        home.path(),
        &["done", "CB-142", "--summary", "Verified and complete"],
    );
    assert!(completed.status.success(), "{}", stderr(&completed));

    let active = aw(home.path(), &[]);
    assert_eq!(stdout(&active), "ACTIVE WORK\n\nNo active work.\n");

    let all = aw(home.path(), &["list", "--all"]);
    let all_listing = stdout(&all);
    assert!(all_listing.contains("[done]"));
    assert!(all_listing.contains("Verified and complete"));
    assert!(!all_listing.contains("Run invoice specs"));
}

#[test]
fn invalid_task_ids_fail_without_writing_outside_the_store() {
    let home = tempdir().unwrap();

    let output = aw(
        home.path(),
        &["start", "../outside", "--title", "Unsafe task"],
    );

    assert!(!output.status.success());
    assert!(stderr(&output).contains("invalid task ID"));
    assert!(!home.path().join("outside.json").exists());
}

#[test]
fn update_requires_at_least_one_changed_cue() {
    let home = tempdir().unwrap();
    let started = aw(home.path(), &["start", "CB-142", "--title", "A task"]);
    assert!(started.status.success(), "{}", stderr(&started));

    let output = aw(home.path(), &["update", "CB-142"]);

    assert!(!output.status.success());
    assert!(stderr(&output).contains("provide at least one"));
}

#[test]
fn start_never_overwrites_an_existing_unreadable_task() {
    let home = tempdir().unwrap();
    let tasks = home.path().join("tasks");
    std::fs::create_dir(&tasks).unwrap();
    let task_path = tasks.join("CB-142.json");
    std::fs::write(&task_path, "not json").unwrap();

    let output = aw(home.path(), &["start", "CB-142", "--title", "A task"]);

    assert!(!output.status.success());
    assert!(stderr(&output).contains("task CB-142 already exists"));
    assert_eq!(std::fs::read_to_string(task_path).unwrap(), "not json");
}

#[test]
fn start_allocates_an_id_and_current_resolves_it_by_worktree() {
    let home = tempdir().unwrap();
    let worktrees = tempdir().unwrap();
    let first = worktrees.path().join("first");
    let second = worktrees.path().join("second");
    std::fs::create_dir_all(&first).unwrap();
    std::fs::create_dir_all(&second).unwrap();

    let started = aw_in(home.path(), &first, &["start", "--title", "First task"]);
    assert!(started.status.success(), "{}", stderr(&started));
    assert_eq!(stdout(&started), "Started AW-0001\n");

    let current = aw_in(home.path(), &first, &["current", "--id-only"]);
    assert!(current.status.success(), "{}", stderr(&current));
    assert_eq!(stdout(&current), "AW-0001\n");

    let duplicate = aw_in(home.path(), &first, &["start", "--title", "Other task"]);
    assert!(!duplicate.status.success());
    assert!(stderr(&duplicate).contains("already active"));

    let next = aw_in(home.path(), &second, &["start", "--title", "Second task"]);
    assert!(next.status.success(), "{}", stderr(&next));
    assert_eq!(stdout(&next), "Started AW-0003\n");
}

#[test]
fn concurrent_agents_receive_unique_ids() {
    let home = tempdir().unwrap();
    let worktrees = tempdir().unwrap();
    let mut children = Vec::new();

    for index in 0..8 {
        let worktree = worktrees.path().join(format!("worktree-{index}"));
        std::fs::create_dir_all(&worktree).unwrap();
        let child = Command::new(env!("CARGO_BIN_EXE_aw"))
            .env("AW_HOME", home.path())
            .current_dir(worktree)
            .args(["start", "--title", &format!("Task {index}")])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        children.push(child);
    }

    let mut ids = HashSet::new();
    for child in children {
        let output = child.wait_with_output().unwrap();
        assert!(output.status.success(), "{}", stderr(&output));
        let id = stdout(&output)
            .trim()
            .strip_prefix("Started ")
            .unwrap()
            .to_owned();
        ids.insert(id);
    }

    assert_eq!(ids.len(), 8);
    assert!(aw(home.path(), &["list", "--all"]).status.success());
}
