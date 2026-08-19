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
        #[arg(long, allow_hyphen_values = true)]
        summary: Option<String>,
        #[arg(long, allow_hyphen_values = true)]
        next: Option<String>,
        /// A decision the user must make before work continues.
        #[arg(long, allow_hyphen_values = true)]
        ask: Option<String>,
        /// Recording agent (claude, codex, ...). Auto-detected from the
        /// environment when omitted.
        #[arg(long)]
        agent: Option<String>,
        /// Conversation/session ID used by `aw resume`. Auto-detected from the
        /// environment when omitted.
        #[arg(long)]
        session_id: Option<String>,
    },
    /// Update the current cue for a task.
    Update {
        id: String,
        #[arg(long)]
        state: Option<State>,
        #[arg(long, allow_hyphen_values = true)]
        summary: Option<String>,
        #[arg(long, allow_hyphen_values = true)]
        next: Option<String>,
        /// A decision the user must make before work continues; pass an empty
        /// string to clear it once resolved.
        #[arg(long, allow_hyphen_values = true)]
        ask: Option<String>,
        /// Recording agent (claude, codex, ...). Auto-detected from the
        /// environment when omitted.
        #[arg(long)]
        agent: Option<String>,
        /// Conversation/session ID used by `aw resume`. Auto-detected from the
        /// environment when omitted.
        #[arg(long)]
        session_id: Option<String>,
    },
    /// Print (or launch) the command that resumes a task's conversation.
    Resume {
        /// Task ID; defaults to the active task of the current worktree.
        id: Option<String>,
        /// Launch the resume command inside the task's directory instead of
        /// printing it.
        #[arg(long)]
        exec: bool,
        /// List every conversation recorded for the task instead of resuming
        /// only the most recent one.
        #[arg(long)]
        list: bool,
    },
    /// Mark a task as done.
    Done {
        id: String,
        #[arg(long, allow_hyphen_values = true)]
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    /// A pending decision the user needs to make before work continues.
    /// Set with `--ask`, cleared with `--ask ""` or `aw done`.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub ask: String,
    /// Every conversation that has written this task. `agent`/`session_id`
    /// above always mirror the most recent writer; this list keeps the rest
    /// from being overwritten.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sessions: Vec<TaskSession>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaskSession {
    pub agent: String,
    pub session_id: String,
    pub last_seen: DateTime<Utc>,
}

impl Task {
    fn refresh_context(&mut self, context: WorkContext) {
        self.directory = context.directory;
        self.branch = context.branch;
        self.dirty = context.dirty;
        self.updated_at = Utc::now();
    }

    fn record_session(&mut self, session: AgentSession) {
        if let (Some(agent), Some(session_id)) = (&session.agent, &session.session_id) {
            let entry = self
                .sessions
                .iter_mut()
                .find(|known| known.agent == *agent && known.session_id == *session_id);
            match entry {
                Some(known) => known.last_seen = Utc::now(),
                None => self.sessions.push(TaskSession {
                    agent: agent.clone(),
                    session_id: session_id.clone(),
                    last_seen: Utc::now(),
                }),
            }
        }
        if session.agent.is_some() {
            self.agent = session.agent;
        }
        if session.session_id.is_some() {
            self.session_id = session.session_id;
        }
    }
}

/// Which agent conversation is writing this task, resolved from explicit flags
/// first, then environment variables (AW_AGENT/AW_SESSION_ID override the
/// agent-specific ones so wrappers can claim authorship).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AgentSession {
    pub agent: Option<String>,
    pub session_id: Option<String>,
}

impl AgentSession {
    pub fn detect(agent: Option<String>, session_id: Option<String>) -> Self {
        let environment = Self::from_environment();
        Self {
            agent: agent.or(environment.agent),
            session_id: session_id.or(environment.session_id),
        }
    }

    fn from_environment() -> Self {
        if env_nonempty("AW_AGENT").is_some() || env_nonempty("AW_SESSION_ID").is_some() {
            return Self {
                agent: env_nonempty("AW_AGENT"),
                session_id: env_nonempty("AW_SESSION_ID"),
            };
        }
        if let Some(session_id) = env_nonempty("CLAUDE_CODE_SESSION_ID") {
            return Self {
                agent: Some("claude".to_owned()),
                session_id: Some(session_id),
            };
        }
        if let Some(session_id) = env_nonempty("CODEX_SESSION_ID") {
            return Self {
                agent: Some("codex".to_owned()),
                session_id: Some(session_id),
            };
        }
        Self::default()
    }
}

fn env_nonempty(key: &str) -> Option<String> {
    env::var(key)
        .ok()
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
}

/// The argv that resumes a conversation for a known agent.
pub fn resume_argv(agent: &str, session_id: &str) -> Result<Vec<String>> {
    let argv: Vec<&str> = match agent {
        "claude" => vec!["claude", "--resume", session_id],
        "codex" => vec!["codex", "resume", session_id],
        other => bail!(
            "no resume recipe for agent {other:?}; its session ID is {session_id} — \
             resume it with that agent's own CLI"
        ),
    };
    Ok(argv.into_iter().map(str::to_owned).collect())
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

    /// Reuse the spelling of an existing project whose name matches
    /// case-insensitively, so "yourclinic" and "YourClinic" never split into
    /// two projects. Best-effort: unreadable task files are skipped rather
    /// than failing the `aw start` that asked.
    pub fn canonical_project_name(&self, requested: String) -> Result<String> {
        let tasks_dir = self.tasks_dir();
        if !tasks_dir.exists() {
            return Ok(requested);
        }
        for entry in fs::read_dir(&tasks_dir)
            .with_context(|| format!("could not read {}", tasks_dir.display()))?
        {
            let path = entry?.path();
            if path.extension() != Some(OsStr::new("json")) {
                continue;
            }
            let Ok(file) = File::open(&path) else {
                continue;
            };
            let Ok(task) = serde_json::from_reader::<_, Task>(BufReader::new(file)) else {
                continue;
            };
            if task.project.eq_ignore_ascii_case(&requested) {
                return Ok(task.project);
            }
        }
        Ok(requested)
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
            ask,
            agent,
            session_id,
        } => {
            let context = WorkContext::detect(current_dir);
            let session = AgentSession::detect(agent, session_id);
            let resolved_project =
                store.canonical_project_name(project.unwrap_or_else(|| context.project.clone()))?;
            let task = store.create(id, |id| {
                let mut task = Task {
                    id,
                    project: resolved_project,
                    title,
                    state: State::Working,
                    summary: summary.unwrap_or_default(),
                    next: next.unwrap_or_default(),
                    ask: ask.unwrap_or_default(),
                    updated_at: Utc::now(),
                    directory: context.directory,
                    branch: context.branch,
                    dirty: context.dirty,
                    agent: None,
                    session_id: None,
                    sessions: Vec::new(),
                };
                task.record_session(session);
                task
            })?;
            Ok(format!("Started {}", task.id))
        }
        Command::Update {
            id,
            state,
            summary,
            next,
            ask,
            agent,
            session_id,
        } => {
            if state.is_none() && summary.is_none() && next.is_none() && ask.is_none() {
                bail!("provide at least one of --state, --summary, --next, or --ask");
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
                if let Some(ask) = ask {
                    task.ask = ask;
                }
                task.record_session(AgentSession::detect(agent, session_id));
                task.refresh_context(WorkContext::detect(current_dir));
                Ok(())
            })?;
            Ok(format!("Updated {id}"))
        }
        Command::Resume { id, exec, list } => {
            let task = match id {
                Some(id) => store.load(&id)?,
                None => {
                    let context = WorkContext::detect(current_dir);
                    let mut tasks = store.active_in_directory(&context.directory)?;
                    match tasks.len() {
                        0 => bail!(
                            "no active task in {}; pass a task ID",
                            context.directory.display()
                        ),
                        1 => tasks.remove(0),
                        _ => {
                            let ids = tasks
                                .iter()
                                .map(|task| task.id.as_str())
                                .collect::<Vec<_>>()
                                .join(", ");
                            bail!("multiple active tasks in this worktree: {ids}; pass a task ID")
                        }
                    }
                }
            };
            if list {
                if task.sessions.is_empty() {
                    bail!("task {} has no recorded conversations", task.id);
                }
                let mut lines = Vec::new();
                for known in task.sessions.iter().rev() {
                    let command = resume_argv(&known.agent, &known.session_id)
                        .map(|argv| argv.join(" "))
                        .unwrap_or_else(|_| {
                            format!("({} session {})", known.agent, known.session_id)
                        });
                    lines.push(format!(
                        "{}  last seen {}  {}",
                        known.agent,
                        known.last_seen.format("%Y-%m-%d %H:%M UTC"),
                        command
                    ));
                }
                return Ok(lines.join("\n"));
            }
            let agent = task.agent.as_deref().with_context(|| {
                format!("task {} has no recorded agent; nothing to resume", task.id)
            })?;
            let session_id = task.session_id.as_deref().with_context(|| {
                format!("task {} has no recorded session ID; nothing to resume", task.id)
            })?;
            let argv = resume_argv(agent, session_id)?;

            if exec {
                #[cfg(unix)]
                {
                    use std::os::unix::process::CommandExt;
                    let error = ProcessCommand::new(&argv[0])
                        .args(&argv[1..])
                        .current_dir(&task.directory)
                        .exec();
                    bail!("could not launch {}: {error}", argv[0]);
                }
                #[cfg(not(unix))]
                bail!("--exec is only supported on unix");
            }
            Ok(format!(
                "cd '{}' && {}",
                task.directory.display(),
                argv.join(" ")
            ))
        }
        Command::Done { id, summary } => {
            store.edit(&id, |task| {
                task.state = State::Done;
                if let Some(summary) = summary {
                    task.summary = summary;
                }
                task.next.clear();
                task.ask.clear();
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
            if tasks.is_empty() {
                bail!(
                    "no active task in {}; run `aw start --title ...`",
                    context.directory.display()
                );
            }
            if id_only {
                Ok(tasks
                    .iter()
                    .map(|task| task.id.as_str())
                    .collect::<Vec<_>>()
                    .join("\n"))
            } else {
                Ok(render_tasks(&tasks, false))
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
        let via = task
            .agent
            .as_deref()
            .map(|agent| format!("  (via {agent})"))
            .unwrap_or_default();
        output.push_str(&format!(
            "\n{} {}  [{}] {} · {}{via}\n",
            task.state.marker(),
            task.id,
            task.state,
            task.project,
            task.title
        ));
        if !task.ask.is_empty() {
            output.push_str(&format!("  ⚑ {}\n", task.ask));
        }
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
            ask: String::new(),
            updated_at: Utc::now(),
            directory: PathBuf::from("/tmp/clinicbase"),
            branch: Some("cb-142".to_owned()),
            dirty: Some(true),
            agent: Some("claude".to_owned()),
            session_id: Some("abc-123".to_owned()),
            sessions: vec![TaskSession {
                agent: "claude".to_owned(),
                session_id: "abc-123".to_owned(),
                last_seen: Utc::now(),
            }],
        };

        store.save(&task).unwrap();

        assert_eq!(store.load("CB-142").unwrap(), task);
    }

    #[test]
    fn loads_tasks_written_before_agent_fields_existed() {
        let directory = tempdir().unwrap();
        let store = Store::new(directory.path());
        let tasks_dir = directory.path().join("tasks");
        fs::create_dir_all(&tasks_dir).unwrap();
        fs::write(
            tasks_dir.join("AW-0001.json"),
            r#"{"id":"AW-0001","project":"demo","title":"Old task","state":"working",
                "summary":"","next":"","updated_at":"2026-01-01T00:00:00Z",
                "directory":"/tmp/demo"}"#,
        )
        .unwrap();

        let task = store.load("AW-0001").unwrap();

        assert_eq!(task.agent, None);
        assert_eq!(task.session_id, None);
        assert!(task.sessions.is_empty());
        assert!(task.ask.is_empty());
    }

    #[test]
    fn project_names_reuse_existing_spelling_case_insensitively() {
        let directory = tempdir().unwrap();
        let store = Store::new(directory.path());
        let task = Task {
            id: "AW-0001".to_owned(),
            project: "YourClinic".to_owned(),
            title: "A task".to_owned(),
            state: State::Working,
            summary: String::new(),
            next: String::new(),
            ask: String::new(),
            updated_at: Utc::now(),
            directory: PathBuf::from("/tmp/demo"),
            branch: None,
            dirty: None,
            agent: None,
            session_id: None,
            sessions: Vec::new(),
        };
        store.save(&task).unwrap();

        let canonical = store
            .canonical_project_name("yourclinic".to_owned())
            .unwrap();
        let fresh = store.canonical_project_name("Landtop".to_owned()).unwrap();

        assert_eq!(canonical, "YourClinic");
        assert_eq!(fresh, "Landtop");
    }

    #[test]
    fn record_session_keeps_every_conversation_without_duplicates() {
        let mut task = Task {
            id: "AW-0001".to_owned(),
            project: "demo".to_owned(),
            title: "A task".to_owned(),
            state: State::Working,
            summary: String::new(),
            next: String::new(),
            ask: String::new(),
            updated_at: Utc::now(),
            directory: PathBuf::from("/tmp/demo"),
            branch: None,
            dirty: None,
            agent: None,
            session_id: None,
            sessions: Vec::new(),
        };

        task.record_session(AgentSession {
            agent: Some("claude".to_owned()),
            session_id: Some("s-1".to_owned()),
        });
        task.record_session(AgentSession {
            agent: Some("codex".to_owned()),
            session_id: Some("s-2".to_owned()),
        });
        task.record_session(AgentSession {
            agent: Some("claude".to_owned()),
            session_id: Some("s-1".to_owned()),
        });

        assert_eq!(task.sessions.len(), 2);
        assert_eq!(task.agent.as_deref(), Some("claude"));
        assert_eq!(task.session_id.as_deref(), Some("s-1"));
    }

    #[test]
    fn resume_argv_knows_claude_and_codex() {
        assert_eq!(
            resume_argv("claude", "abc").unwrap(),
            vec!["claude", "--resume", "abc"]
        );
        assert_eq!(
            resume_argv("codex", "abc").unwrap(),
            vec!["codex", "resume", "abc"]
        );

        let error = resume_argv("agy", "abc").unwrap_err().to_string();
        assert!(error.contains("no resume recipe"));
        assert!(error.contains("abc"));
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
            ask: "Decide: A or B?".to_owned(),
            updated_at: Utc::now(),
            directory: PathBuf::from("/tmp/clinicbase"),
            branch: None,
            dirty: None,
            agent: Some("claude".to_owned()),
            session_id: None,
            sessions: Vec::new(),
        };

        let output = render_tasks(&[task], false);

        assert!(output.contains("Found the rounding point"));
        assert!(output.contains("→ Run invoice specs"));
        assert!(output.contains("(via claude)"));
        assert!(output.contains("⚑ Decide: A or B?"));
    }
}
