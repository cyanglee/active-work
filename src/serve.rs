use std::io::Cursor;

use anyhow::{Context, Result, anyhow, bail};
use chrono::Utc;
use tiny_http::{Header, Method, Response, Server};

use crate::{State, Store};

const BOARD_HTML: &str = include_str!("board.html");

pub fn run(store: &Store, port: u16) -> Result<()> {
    let address = format!("127.0.0.1:{port}");
    let server =
        Server::http(&address).map_err(|error| anyhow!("could not bind {address}: {error}"))?;
    println!("aw board on http://{address} (Ctrl-C to stop)");

    for request in server.incoming_requests() {
        let url = request.url().to_owned();
        let (path, query) = url.split_once('?').unwrap_or((url.as_str(), ""));
        let response = match (request.method(), path) {
            (Method::Get, "/" | "/index.html") => {
                page(BOARD_HTML.to_owned(), "text/html; charset=utf-8", 200)
            }
            (Method::Get, "/tasks.json") => match tasks_json(store) {
                Ok(payload) => page(payload, "application/json", 200),
                Err(error) => error_page(error),
            },
            // The X-AW header forces a CORS preflight, which this server never
            // answers — so cross-origin pages cannot fire these state changes.
            (Method::Post, "/done") if has_aw_header(&request) => {
                match set_state(store, query, State::Done) {
                    Ok(payload) => page(payload, "application/json", 200),
                    Err(error) => error_page(error),
                }
            }
            (Method::Post, "/reopen") if has_aw_header(&request) => {
                match set_state(store, query, State::Working) {
                    Ok(payload) => page(payload, "application/json", 200),
                    Err(error) => error_page(error),
                }
            }
            (Method::Post, "/done" | "/reopen") => {
                page("missing X-AW header".to_owned(), "text/plain", 403)
            }
            _ => page("not found".to_owned(), "text/plain; charset=utf-8", 404),
        };
        let _ = request.respond(response);
    }
    Ok(())
}

/// Handle a board button: flip a task's state without touching its worktree
/// binding (the server's cwd is unrelated to the task's directory).
pub fn set_state(store: &Store, query: &str, state: State) -> Result<String> {
    let id = query
        .split('&')
        .find_map(|pair| pair.strip_prefix("id="))
        .filter(|id| !id.is_empty())
        .map(str::to_owned);
    let Some(id) = id else {
        bail!("missing id parameter");
    };
    let task = store.edit(&id, |task| {
        task.state = state;
        if state == State::Done {
            task.next.clear();
            task.ask.clear();
        }
        task.updated_at = Utc::now();
        Ok(())
    })?;
    serde_json::to_string(&task).context("could not serialize task")
}

fn has_aw_header(request: &tiny_http::Request) -> bool {
    request
        .headers()
        .iter()
        .any(|header| header.field.equiv("x-aw"))
}

fn error_page(error: anyhow::Error) -> Response<Cursor<Vec<u8>>> {
    let body = serde_json::json!({ "error": error.to_string() }).to_string();
    page(body, "application/json", 500)
}

pub fn tasks_json(store: &Store) -> Result<String> {
    let mut tasks = Vec::new();
    for task in store.list()? {
        let pings = store.pings(&task.id).unwrap_or_default();
        let mut value = serde_json::to_value(&task).context("could not serialize task")?;
        if let Some(object) = value.as_object_mut() {
            object.insert(
                "time_seconds".to_owned(),
                crate::active_seconds(&pings, 300).into(),
            );
            object.insert("last_ping".to_owned(), pings.last().copied().into());
        }
        tasks.push(value);
    }
    let payload = serde_json::json!({
        "generated_at": Utc::now(),
        "tasks": tasks,
    });
    serde_json::to_string(&payload).context("could not serialize tasks")
}

fn page(body: String, content_type: &str, status: u16) -> Response<Cursor<Vec<u8>>> {
    Response::from_string(body)
        .with_status_code(status)
        .with_header(
            Header::from_bytes(&b"Content-Type"[..], content_type.as_bytes())
                .expect("static header is valid"),
        )
        .with_header(
            Header::from_bytes(&b"Cache-Control"[..], &b"no-store"[..])
                .expect("static header is valid"),
        )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{State, Task};
    use std::path::PathBuf;
    use tempfile::tempdir;

    fn task(id: &str, state: State) -> Task {
        Task {
            id: id.to_owned(),
            project: "ClinicBase".to_owned(),
            title: "Fix invoice rounding".to_owned(),
            state,
            summary: "Found the rounding point".to_owned(),
            next: "Run invoice specs".to_owned(),
            ask: String::new(),
            updated_at: Utc::now(),
            directory: PathBuf::from("/tmp/clinicbase"),
            branch: Some("cb-142".to_owned()),
            dirty: Some(true),
            agent: None,
            session_id: None,
            sessions: Vec::new(),
        }
    }

    #[test]
    fn board_done_button_completes_a_task_and_clears_its_cues() {
        let directory = tempdir().unwrap();
        let store = Store::new(directory.path());
        let mut fixture = task("CB-1", State::Review);
        fixture.ask = "A or B?".to_owned();
        store.save(&fixture).unwrap();

        set_state(&store, "id=CB-1", State::Done).unwrap();

        let done = store.load("CB-1").unwrap();
        assert_eq!(done.state, State::Done);
        assert!(done.next.is_empty());
        assert!(done.ask.is_empty());

        set_state(&store, "id=CB-1", State::Working).unwrap();
        assert_eq!(store.load("CB-1").unwrap().state, State::Working);

        let missing = set_state(&store, "", State::Done).unwrap_err();
        assert!(missing.to_string().contains("missing id"));
    }

    #[test]
    fn tasks_json_keeps_done_tasks_so_the_board_can_toggle_them() {
        let directory = tempdir().unwrap();
        let store = Store::new(directory.path());
        store.save(&task("CB-1", State::Working)).unwrap();
        store.save(&task("CB-2", State::Done)).unwrap();

        let payload = tasks_json(&store).unwrap();

        let value: serde_json::Value = serde_json::from_str(&payload).unwrap();
        assert_eq!(value["tasks"].as_array().unwrap().len(), 2);
        assert!(value["generated_at"].is_string());
    }
}
