use std::io::Write;
use std::thread;
use std::time::Duration;

use anyhow::Result;
use chrono::{DateTime, Local, Utc};

use crate::{State, Store, Task};

const CLEAR: &str = "\x1b[2J\x1b[H";
const RESET: &str = "\x1b[0m";
const BOLD: &str = "\x1b[1m";
const DIM: &str = "\x1b[2m";

pub fn run(store: &Store, interval_seconds: u64, all: bool) -> Result<()> {
    let interval = Duration::from_secs(interval_seconds.max(1));
    loop {
        let body = match store.list() {
            Ok(tasks) => render_watch(&tasks, all, Utc::now()),
            Err(error) => format!("could not read tasks: {error:#}"),
        };
        let clock = Local::now().format("%H:%M:%S");
        print!(
            "{CLEAR}{DIM}aw watch · {clock} · every {}s · Ctrl-C to quit{RESET}\n\n{body}\n",
            interval.as_secs()
        );
        std::io::stdout().flush()?;
        thread::sleep(interval);
    }
}

pub fn render_watch(tasks: &[Task], all: bool, now: DateTime<Utc>) -> String {
    let visible = tasks
        .iter()
        .filter(|task| all || task.state != State::Done)
        .collect::<Vec<_>>();
    if visible.is_empty() {
        let empty_message = if all { "No work." } else { "No active work." };
        return empty_message.to_owned();
    }

    // Tasks arrive sorted by recency; groups keep the order of first appearance.
    let mut groups: Vec<(&str, Vec<&Task>)> = Vec::new();
    for task in visible {
        match groups.iter_mut().find(|(name, _)| *name == task.project) {
            Some((_, list)) => list.push(task),
            None => groups.push((&task.project, vec![task])),
        }
    }

    let mut output = String::new();
    for (project, list) in groups {
        output.push_str(&format!("{BOLD}{project}{RESET} {DIM}({}){RESET}\n", list.len()));
        for task in list {
            let color = state_color(task.state);
            output.push_str(&format!(
                "  {color}{} {}{RESET} {color}[{}]{RESET} {BOLD}{}{RESET} {DIM}({}){RESET}\n",
                task.state.marker(),
                task.id,
                task.state,
                task.title,
                relative_age(task.updated_at, now)
            ));
            if !task.summary.is_empty() {
                output.push_str(&format!("      {}\n", task.summary));
            }
            if !task.next.is_empty() {
                output.push_str(&format!("      {color}→{RESET} {}\n", task.next));
            }
            if let Some(branch) = &task.branch {
                let dirty = if task.dirty == Some(true) { "*" } else { "" };
                output.push_str(&format!("      {DIM}⎇ {branch}{dirty}{RESET}\n"));
            }
        }
        output.push('\n');
    }
    output.trim_end().to_owned()
}

fn state_color(state: State) -> &'static str {
    match state {
        State::Working => "\x1b[32m",
        State::Waiting => "\x1b[33m",
        State::Review => "\x1b[35m",
        State::Done => "\x1b[90m",
    }
}

fn relative_age(updated_at: DateTime<Utc>, now: DateTime<Utc>) -> String {
    let seconds = (now - updated_at).num_seconds().max(0);
    match seconds {
        0..60 => "just now".to_owned(),
        60..3600 => format!("{}m ago", seconds / 60),
        3600..86400 => format!("{}h ago", seconds / 3600),
        _ => format!("{}d ago", seconds / 86400),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeDelta;
    use std::path::PathBuf;

    fn task(id: &str, project: &str, state: State) -> Task {
        Task {
            id: id.to_owned(),
            project: project.to_owned(),
            title: "Fix invoice rounding".to_owned(),
            state,
            summary: "Found the rounding point".to_owned(),
            next: "Run invoice specs".to_owned(),
            updated_at: Utc::now(),
            directory: PathBuf::from("/tmp/clinicbase"),
            branch: Some("cb-142".to_owned()),
            dirty: Some(true),
        }
    }

    #[test]
    fn groups_tasks_under_a_single_project_heading() {
        let tasks = [
            task("CB-1", "ClinicBase", State::Working),
            task("CB-2", "ClinicBase", State::Review),
        ];

        let output = render_watch(&tasks, false, Utc::now());

        assert_eq!(output.matches("ClinicBase").count(), 1);
        assert!(output.contains("CB-1"));
        assert!(output.contains("CB-2"));
    }

    #[test]
    fn hides_done_tasks_unless_all_is_requested() {
        let tasks = [
            task("CB-1", "ClinicBase", State::Working),
            task("CB-2", "ClinicBase", State::Done),
        ];

        assert!(!render_watch(&tasks, false, Utc::now()).contains("CB-2"));
        assert!(render_watch(&tasks, true, Utc::now()).contains("CB-2"));
    }

    #[test]
    fn states_get_distinct_colors() {
        let output = render_watch(
            &[
                task("CB-1", "ClinicBase", State::Working),
                task("CB-2", "ClinicBase", State::Waiting),
            ],
            false,
            Utc::now(),
        );

        assert!(output.contains("\x1b[32m"));
        assert!(output.contains("\x1b[33m"));
    }

    #[test]
    fn ages_are_humanized() {
        let now = Utc::now();
        let five_minutes_ago = now - TimeDelta::minutes(5);
        let two_hours_ago = now - TimeDelta::hours(2);

        assert_eq!(relative_age(now, now), "just now");
        assert_eq!(relative_age(five_minutes_ago, now), "5m ago");
        assert_eq!(relative_age(two_hours_ago, now), "2h ago");
    }
}
