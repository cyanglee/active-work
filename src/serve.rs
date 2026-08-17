use std::io::Cursor;

use anyhow::{Context, Result, anyhow};
use chrono::Utc;
use tiny_http::{Header, Response, Server};

use crate::Store;

const BOARD_HTML: &str = include_str!("board.html");

pub fn run(store: &Store, port: u16) -> Result<()> {
    let address = format!("127.0.0.1:{port}");
    let server =
        Server::http(&address).map_err(|error| anyhow!("could not bind {address}: {error}"))?;
    println!("aw board on http://{address} (Ctrl-C to stop)");

    for request in server.incoming_requests() {
        let response = match request.url() {
            "/" | "/index.html" => page(BOARD_HTML.to_owned(), "text/html; charset=utf-8", 200),
            "/tasks.json" => match tasks_json(store) {
                Ok(payload) => page(payload, "application/json", 200),
                Err(error) => {
                    let body = serde_json::json!({ "error": error.to_string() }).to_string();
                    page(body, "application/json", 500)
                }
            },
            _ => page("not found".to_owned(), "text/plain; charset=utf-8", 404),
        };
        let _ = request.respond(response);
    }
    Ok(())
}

pub fn tasks_json(store: &Store) -> Result<String> {
    let payload = serde_json::json!({
        "generated_at": Utc::now(),
        "tasks": store.list()?,
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
            updated_at: Utc::now(),
            directory: PathBuf::from("/tmp/clinicbase"),
            branch: Some("cb-142".to_owned()),
            dirty: Some(true),
        }
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
