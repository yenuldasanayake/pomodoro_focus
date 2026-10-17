use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use pomodoro_focus::server::{build_router, generate_pin, AppState, SharedState};
use pomodoro_focus::storage::Storage;
use std::{
    net::SocketAddr,
    path::PathBuf,
    sync::{Arc, Mutex},
};
use tower::ServiceExt;

fn temp_root(name: &str) -> PathBuf {
    let mut path = std::env::temp_dir();
    let unique = format!(
        "pomodoro-server-test-{}-{}",
        name,
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    );
    path.push(unique);
    path
}

fn test_state(name: &str) -> (SharedState, PathBuf, String) {
    let root = temp_root(name);
    let storage = Storage::new(&root);
    let ctx = eframe::egui::Context::default();
    let state = AppState::new(ctx, storage).expect("AppState::new");
    let pin = state.pin.clone();
    (Arc::new(Mutex::new(state)), root, pin)
}

/// A client IP for tests. We rely on `ConnectInfo` being populated.
fn addr() -> SocketAddr {
    SocketAddr::from(([127, 0, 0, 1], 40000))
}

async fn body_json<T: serde::de::DeserializeOwned>(body: Body) -> T {
    let bytes = body.collect().await.unwrap().to_bytes();
    serde_json::from_slice(&bytes).unwrap()
}

#[tokio::test]
async fn status_returns_shape() {
    let (state, root, _pin) = test_state("status");
    let app = build_router(state);

    let response = app
        .oneshot(
            Request::builder()
                .uri("/status")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let json: serde_json::Value = body_json(response.into_body()).await;
    assert!(json.get("remaining_seconds").is_some());
    assert!(json.get("mode").is_some());
    assert!(json.get("notes").is_some());

    let _ = std::fs::remove_dir_all(&root);
}

#[tokio::test]
async fn command_rejects_wrong_pin() {
    let (state, root, _pin) = test_state("wrong-pin");
    let app = build_router(state);

    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/command")
                .header("Content-Type", "application/json")
                .extension(axum::extract::ConnectInfo(addr()))
                .body(Body::from(r#"{"token":"00000000","command":"start"}"#))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);

    let _ = std::fs::remove_dir_all(&root);
}

#[tokio::test]
async fn command_accepts_correct_pin() {
    let (state, root, pin) = test_state("ok-pin");
    let app = build_router(state);

    let body = format!(r#"{{"token":"{pin}","command":"start"}}"#);
    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/command")
                .header("Content-Type", "application/json")
                .extension(axum::extract::ConnectInfo(addr()))
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let json: serde_json::Value = body_json(response.into_body()).await;
    assert_eq!(json["ok"], true);

    let _ = std::fs::remove_dir_all(&root);
}

#[tokio::test]
async fn rate_limit_locks_after_five_failures() {
    let (state, root, _pin) = test_state("rate-limit");
    let app = build_router(state.clone());

    // Fire five wrong-PIN requests from the same IP.
    for _ in 0..5 {
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/command")
                    .header("Content-Type", "application/json")
                    .extension(axum::extract::ConnectInfo(addr()))
                    .body(Body::from(r#"{"token":"00000000","command":"start"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }

    // Sixth attempt should be 429, even with the correct PIN.
    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/command")
                .header("Content-Type", "application/json")
                .extension(axum::extract::ConnectInfo(addr()))
                .body(Body::from(format!(
                    r#"{{"token":"{}","command":"start"}}"#,
                    state.lock().unwrap().pin
                )))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);

    let _ = std::fs::remove_dir_all(&root);
}

#[tokio::test]
async fn add_note_returns_new_id() {
    let (state, root, pin) = test_state("add-note");
    let app = build_router(state);

    let body = format!(
        r#"{{"token":"{pin}","accomplishment":"Wrote tests","subject":"Rust"}}"#
    );
    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/notes")
                .header("Content-Type", "application/json")
                .extension(axum::extract::ConnectInfo(addr()))
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let json: serde_json::Value = body_json(response.into_body()).await;
    assert_eq!(json["ok"], true);
    let id = json["id"].as_str().unwrap();
    assert_eq!(id.len(), 32);

    let _ = std::fs::remove_dir_all(&root);
}

#[tokio::test]
async fn update_note_with_unknown_id_returns_400() {
    let (state, root, pin) = test_state("update-unknown");
    let app = build_router(state);

    let body = format!(
        r#"{{"token":"{pin}","id":"nonexistent","accomplishment":"x","subject":""}}"#
    );
    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/notes/update")
                .header("Content-Type", "application/json")
                .extension(axum::extract::ConnectInfo(addr()))
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);

    let _ = std::fs::remove_dir_all(&root);
}

#[tokio::test]
async fn delete_note_is_idempotent() {
    let (state, root, pin) = test_state("delete-idem");
    let app = build_router(state);

    // Create a note.
    let body = format!(r#"{{"token":"{pin}","accomplishment":"to delete","subject":""}}"#);
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/notes")
                .header("Content-Type", "application/json")
                .extension(axum::extract::ConnectInfo(addr()))
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap();
    let json: serde_json::Value = body_json(response.into_body()).await;
    let id = json["id"].as_str().unwrap().to_owned();

    // Delete it.
    let body = format!(r#"{{"token":"{pin}","id":"{id}"}}"#);
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/notes/delete")
                .header("Content-Type", "application/json")
                .extension(axum::extract::ConnectInfo(addr()))
                .body(Body::from(body.clone()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    // Delete again — should be 400, "Note not found".
    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/notes/delete")
                .header("Content-Type", "application/json")
                .extension(axum::extract::ConnectInfo(addr()))
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);

    let _ = std::fs::remove_dir_all(&root);
}

// Note: `generate_pin` is imported but only used to silence the unused-import
// warning if the tests above change. Remove the import if clippy complains.
#[allow(dead_code)]
fn _keep_generate_pin_used() -> Result<String, getrandom::Error> {
    generate_pin()
}