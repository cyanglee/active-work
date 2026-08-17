use std::env;
use std::ffi::OsStr;
use std::fmt;
use std::fs::{self, File, OpenOptions};
use std::io::{BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::Command as ProcessCommand;

use anyhow::{Context, Result, bail};
use chrono::{DateTime, Utc};
use clap::{Parser, Subcommand, ValueEnum};
use directories::UserDirs;
use fs2::FileExt;
use serde::{Deserialize, Serialize};
use tempfile::NamedTempFile;

pub mod serve;
pub mod watch;

#[derive(Debug, Parser)]
#[command(name = "aw", version, about = "A small cue board for active work")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Command>,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Start a task in the current directory.
    Start {
        id: Option<String>,
        #[arg(long)]
        title: String,
        #[arg(long)]
        project: Option<String>,
        #[arg(long)]
        summary: Option<String>,
        #[arg(long)]
        next: Option<String>,
    },
    /// Update the current cue for a task.
    Update {
        id: String,
        #[arg(long)]
        state: Option<State>,
        #[arg(long)]
        summary: Option<String>,
        #[arg(long)]
        next: Option<String>,
    },
    /// Mark a task as done.
    Done {
        id: String,
        #[arg(long)]
        summary: Option<String>,
    },
    /// List tasks. Done tasks are hidden unless --all is supplied.
    List {
        #[arg(long)]
        all: bool,
    },
    /// Show the active task for the current worktree.
    Current {
        #[arg(long)]
        id_only: bool,
    },
    /// Serve a live web board on localhost.
    Serve {
        #[arg(long, default_value_t = 7337)]
        port: u16,
    },
    /// Watch tasks in the terminal, redrawing every few seconds.
    Watch {
        /// Refresh interval in seconds.
        #[arg(long, default_value_t = 2)]
        interval: u64,
        #[arg(long)]
        all: bool,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ValueEnum)]
#[serde(rename_all = "snake_case")]
pub enum State {
    Working,
    Waiting,
    Review,
    Done,
}

impl State {
    fn marker(self) -> &'static str {
        match self {
            Self::Working => "●",
            Self::Waiting => "◐",
            Self::Review => "◆",
            Self::Done => "✓",
        }
    }
}

impl fmt::Display for State {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let value = match self {
            Self::Working => "working",
            Self::Waiting => "waiting",
            Self::Review => "review",
            Self::Done => "done",
        };
        formatter.write_str(value)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Task {
    pub id: String,
    pub project: String,
    pub title: String,
    pub state: State,
    pub summary: String,
    pub next: String,
    pub updated_at: DateTime<Utc>,
    pub directory: PathBuf,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub branch: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dirty: Option<bool>,
}

impl Task {
    fn refresh_context(&mut self, context: WorkContext) {
        self.directory = context.directory;
        self.branch = context.branch;
        self.dirty = context.dirty;
        self.updated_at = Utc::now();
    }
}

#[derive(Debug)]
struct WorkContext {
    directory: PathBuf,
    project: String,
    branch: Option<String>,
    dirty: Option<bool>,
}

impl WorkContext {
    fn detect(current_dir: &Path) -> Self {
        let git_root = git_output(current_dir, ["rev-parse", "--show-toplevel"]).map(PathBuf::from);
        let directory = git_root.unwrap_or_else(|| current_dir.to_path_buf());
        let project = directory
            .file_name()
            .and_then(OsStr::to_str)
            .filter(|name| !name.is_empty())
            .unwrap_or("work")
            .to_owned();
        let branch =
            git_output(current_dir, ["branch", "--show-current"]).filter(|value| !value.is_empty());
        let dirty =
            git_output(current_dir, ["status", "--porcelain"]).map(|value| !value.is_empty());

        Self {
            directory,
            project,
            branch,
            dirty,
        }
    }
}

fn git_output<const N: usize>(current_dir: &Path, args: [&str; N]) -> Option<String> {
    let output = ProcessCommand::new("git")
        .args(args)
        .current_dir(current_dir)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }

    String::from_utf8(output.stdout)
        .ok()
        .map(|value| value.trim().to_owned())
}

#[derive(Debug, Clone)]
pub struct Store {
    root: PathBuf,
}

impl Store {
    pub fn from_environment() -> Result<Self> {
        if let Some(path) = env::var_os("AW_HOME") {
            if path.is_empty() {
                bail!("AW_HOME cannot be empty");
            }
            return Ok(Self::new(path));
        }

        let user_dirs = UserDirs::new().context("could not determine the home directory")?;
        Ok(Self::new(user_dirs.home_dir().join(".agent-work")))
    }

    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    fn tasks_dir(&self) -> PathBuf {
        self.root.join("tasks")
    }

    fn lock_path(&self) -> PathBuf {
        self.root.join(".lock")
    }

    fn sequence_path(&self) -> PathBuf {
        self.root.join(".next-id")
    }

    fn task_path(&self, id: &str) -> Result<PathBuf> {
        validate_task_id(id)?;
        Ok(self.tasks_dir().join(format!("{id}.json")))
    }

    pub fn contains(&self, id: &str) -> Result<bool> {
        Ok(self.task_path(id)?.exists())
    }

    pub fn create<F>(&self, requested_id: Option<String>, build: F) -> Result<Task>
    where
        F: FnOnce(String) -> Task,
    {
        self.with_exclusive_lock(|| {
            let id = match requested_id {
                Some(id) => {
                    validate_task_id(&id)?;
                    id
                }
                None => self.next_id_unlocked()?,
            };
            if self.contains(&id)? {
                bail!("task {id} already exists; use `aw update {id}`");
            }

            let task = build(id);
            let active = self.active_in_directory_unlocked(&task.directory)?;
            if let Some(existing) = active.first() {
                bail!(
                    "task {} is already active in {}; use `aw current` or a separate worktree",
                    existing.id,
                    task.directory.display()
                );
            }
            self.save_unlocked(&task)?;
            Ok(task)
        })
    }

    pub fn edit<F>(&self, id: &str, edit: F) -> Result<Task>
    where
        F: FnOnce(&mut Task) -> Result<()>,
    {
        self.with_exclusive_lock(|| {
            let mut task = self.load(id)?;
            edit(&mut task)?;
            self.save_unlocked(&task)?;
            Ok(task)
        })
    }

    pub fn load(&self, id: &str) -> Result<Task> {
        let path = self.task_path(id)?;
        let file = File::open(&path).with_context(|| format!("task {id} does not exist"))?;
        serde_json::from_reader(BufReader::new(file))
            .with_context(|| format!("could not read {}", path.display()))
    }

    pub fn save(&self, task: &Task) -> Result<()> {
        self.with_exclusive_lock(|| self.save_unlocked(task))
    }

    fn save_unlocked(&self, task: &Task) -> Result<()> {
        let path = self.task_path(&task.id)?;
        let tasks_dir = self.tasks_dir();
        fs::create_dir_all(&tasks_dir)
            .with_context(|| format!("could not create {}", tasks_dir.display()))?;

        let mut temporary = NamedTempFile::new_in(&tasks_dir).with_context(|| {
            format!(
                "could not create a temporary file in {}",
                tasks_dir.display()
            )
        })?;
        serde_json::to_writer_pretty(&mut temporary, task).context("could not serialize task")?;
        temporary.write_all(b"\n")?;
        temporary.as_file().sync_all()?;
        temporary
            .persist(&path)
            .map_err(|error| error.error)
            .with_context(|| format!("could not replace {}", path.display()))?;
        Ok(())
    }

    fn with_exclusive_lock<T>(&self, operation: impl FnOnce() -> Result<T>) -> Result<T> {
        fs::create_dir_all(&self.root)
            .with_context(|| format!("could not create {}", self.root.display()))?;
        let lock = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(self.lock_path())
            .context("could not open the aw store lock")?;
        lock.lock_exclusive()
            .context("could not lock the aw store")?;
        let result = operation();
        FileExt::unlock(&lock).context("could not unlock the aw store")?;
        result
    }

    fn next_id_unlocked(&self) -> Result<String> {
        let sequence_path = self.sequence_path();
        let last = if sequence_path.exists() {
            let value = fs::read_to_string(&sequence_path)
                .with_context(|| format!("could not read {}", sequence_path.display()))?;
            value
                .trim()
                .parse::<u64>()
                .with_context(|| format!("invalid sequence in {}", sequence_path.display()))?
        } else {
            0
        };

        let mut next = last.checked_add(1).context("task ID sequence exhausted")?;
        loop {
            let id = format!("AW-{next:04}");
            if !self.contains(&id)? {
                write_atomic(&sequence_path, format!("{next}\n").as_bytes())?;
                return Ok(id);
            }
            next = next.checked_add(1).context("task ID sequence exhausted")?;
        }
    }

    pub fn list(&self) -> Result<Vec<Task>> {
        let tasks_dir = self.tasks_dir();
        if !tasks_dir.exists() {
            return Ok(Vec::new());
        }

        let mut tasks = Vec::new();
        for entry in fs::read_dir(&tasks_dir)
            .with_context(|| format!("could not read {}", tasks_dir.display()))?
        {
            let path = entry?.path();
            if path.extension() != Some(OsStr::new("json")) {
                continue;
            }
            let file = File::open(&path)?;
            let task = serde_json::from_reader(BufReader::new(file))
                .with_context(|| format!("could not read {}", path.display()))?;
            tasks.push(task);
        }
        tasks.sort_by(|left: &Task, right: &Task| right.updated_at.cmp(&left.updated_at));
        Ok(tasks)
    }

    pub fn active_in_directory(&self, directory: &Path) -> Result<Vec<Task>> {
        self.active_in_directory_unlocked(directory)
    }

    fn active_in_directory_unlocked(&self, directory: &Path) -> Result<Vec<Task>> {
        Ok(self
            .list()?
            .into_iter()
            .filter(|task| task.state != State::Done && task.directory == directory)
            .collect())
    }
}

fn write_atomic(path: &Path, contents: &[u8]) -> Result<()> {
    let parent = path
        .parent()
        .context("atomic write needs a parent directory")?;
    fs::create_dir_all(parent)?;
    let mut temporary = NamedTempFile::new_in(parent)?;
    temporary.write_all(contents)?;
    temporary.as_file().sync_all()?;
    temporary
        .persist(path)
        .map_err(|error| error.error)
        .with_context(|| format!("could not replace {}", path.display()))?;
    Ok(())
}

pub fn execute(command: Option<Command>, store: &Store, current_dir: &Path) -> Result<String> {
    match command.unwrap_or(Command::List { all: false }) {
        Command::Start {
            id,
            title,
            project,
            summary,
            next,
        } => {
            let context = WorkContext::detect(current_dir);
            let resolved_project = project.unwrap_or_else(|| context.project.clone());
            let task = store.create(id, |id| Task {
                id,
                project: resolved_project,
                title,
                state: State::Working,
                summary: summary.unwrap_or_default(),
                next: next.unwrap_or_default(),
                updated_at: Utc::now(),
                directory: context.directory,
                branch: context.branch,
                dirty: context.dirty,
            })?;
            Ok(format!("Started {}", task.id))
        }
        Command::Update {
            id,
            state,
            summary,
            next,
        } => {
            if state.is_none() && summary.is_none() && next.is_none() {
                bail!("provide at least one of --state, --summary, or --next");
            }

            store.edit(&id, |task| {
                if let Some(state) = state {
                    task.state = state;
                }
                if let Some(summary) = summary {
                    task.summary = summary;
                }
                if let Some(next) = next {
                    task.next = next;
                }
                task.refresh_context(WorkContext::detect(current_dir));
                Ok(())
            })?;
            Ok(format!("Updated {id}"))
        }
        Command::Done { id, summary } => {
            store.edit(&id, |task| {
                task.state = State::Done;
                if let Some(summary) = summary {
                    task.summary = summary;
                }
                task.next.clear();
                task.refresh_context(WorkContext::detect(current_dir));
                Ok(())
            })?;
            Ok(format!("Completed {id}"))
        }
        Command::List { all } => {
            let tasks = store
                .list()?
                .into_iter()
                .filter(|task| all || task.state != State::Done)
                .collect::<Vec<_>>();
            Ok(render_tasks(&tasks, all))
        }
        Command::Current { id_only } => {
            let context = WorkContext::detect(current_dir);
            let tasks = store.active_in_directory(&context.directory)?;
            match tasks.as_slice() {
                [] => bail!(
                    "no active task in {}; run `aw start --title ...`",
                    context.directory.display()
                ),
                [task] if id_only => Ok(task.id.clone()),
                [task] => Ok(render_tasks(std::slice::from_ref(task), false)),
                tasks => {
                    let ids = tasks
                        .iter()
                        .map(|task| task.id.as_str())
                        .collect::<Vec<_>>()
                        .join(", ");
                    bail!("multiple active tasks in this worktree: {ids}; specify a task ID")
                }
            }
        }
        // Long-running commands are dispatched in main before execute is reached.
        Command::Serve { .. } => bail!("`aw serve` runs until interrupted; start it directly"),
        Command::Watch { .. } => bail!("`aw watch` runs until interrupted; start it directly"),
    }
}

pub fn render_tasks(tasks: &[Task], all: bool) -> String {
    let heading = if all { "ALL WORK" } else { "ACTIVE WORK" };
    if tasks.is_empty() {
        let empty_message = if all { "No work." } else { "No active work." };
        return format!("{heading}\n\n{empty_message}");
    }

    let mut output = format!("{heading}\n");
    for task in tasks {
        output.push_str(&format!(
            "\n{} {}  [{}] {} · {}\n",
            task.state.marker(),
            task.id,
            task.state,
            task.project,
            task.title
        ));
        if !task.summary.is_empty() {
            output.push_str(&format!("  {}\n", task.summary));
        }
        if !task.next.is_empty() {
            output.push_str(&format!("  → {}\n", task.next));
        }
    }
    output.trim_end().to_owned()
}

fn validate_task_id(id: &str) -> Result<()> {
    let valid = !id.is_empty()
        && !id.starts_with('.')
        && id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'));
    if !valid {
        bail!("invalid task ID {id:?}; use letters, numbers, '-', '_', or '.'");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn store_round_trips_a_task() {
        let directory = tempdir().unwrap();
        let store = Store::new(directory.path());
        let task = Task {
            id: "CB-142".to_owned(),
            project: "ClinicBase".to_owned(),
            title: "Fix invoice rounding".to_owned(),
            state: State::Working,
            summary: "Found the rounding point".to_owned(),
            next: "Run invoice specs".to_owned(),
            updated_at: Utc::now(),
            directory: PathBuf::from("/tmp/clinicbase"),
            branch: Some("cb-142".to_owned()),
            dirty: Some(true),
        };

        store.save(&task).unwrap();

        assert_eq!(store.load("CB-142").unwrap(), task);
    }

    #[test]
    fn rejects_task_ids_that_can_escape_the_store() {
        let directory = tempdir().unwrap();
        let store = Store::new(directory.path());

        let error = store.load("../outside").unwrap_err().to_string();

        assert!(error.contains("invalid task ID"));
    }

    #[test]
    fn render_uses_summary_and_next_as_the_primary_cues() {
        let task = Task {
            id: "CB-142".to_owned(),
            project: "ClinicBase".to_owned(),
            title: "Fix invoice rounding".to_owned(),
            state: State::Working,
            summary: "Found the rounding point".to_owned(),
            next: "Run invoice specs".to_owned(),
            updated_at: Utc::now(),
            directory: PathBuf::from("/tmp/clinicbase"),
            branch: None,
            dirty: None,
        };

        let output = render_tasks(&[task], false);

        assert!(output.contains("Found the rounding point"));
        assert!(output.contains("→ Run invoice specs"));
    }
}
