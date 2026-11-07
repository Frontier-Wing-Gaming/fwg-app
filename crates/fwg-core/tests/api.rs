//! Request/response behaviour against a local mock server, plus an opt-in live smoke test.

use std::sync::Arc;

use fwg_core::{
    ApiClient, ApiError, Error, ImageUpload, Session,
    auth::SESSION_EXPIRED,
    storage::{KeyValueStore, MemoryStore},
};
use mockito::{Matcher, Server};

fn client(server: &Server) -> ApiClient {
    ApiClient::new(&format!("{}/api/v1", server.url()))
}

#[tokio::test]
async fn public_request_sends_json_headers_and_parses() {
    let mut server = Server::new_async().await;
    let m = server
        .mock("GET", "/api/v1/news.php")
        .match_header("accept", "application/json")
        .match_header("authorization", Matcher::Missing)
        .with_header("content-type", "application/json; charset=utf-8")
        .with_body(r#"{"items":[{"id":1,"title":"Hi","body":"b","published_at":"2026-11-01"}]}"#)
        .create_async()
        .await;
    let news = client(&server).news().await.unwrap();
    assert_eq!(news.items[0].title, "Hi");
    m.assert_async().await;
}

#[tokio::test]
async fn error_field_becomes_message() {
    let mut server = Server::new_async().await;
    server
        .mock("POST", "/api/v1/auth/login.php")
        .match_body(Matcher::Json(
            serde_json::json!({"email":"a@b.c","password":"pw"}),
        ))
        .with_status(422)
        .with_header("content-type", "application/json")
        .with_body(r#"{"error":"Invalid email or password."}"#)
        .create_async()
        .await;
    let err = client(&server).login("a@b.c", "pw").await.unwrap_err();
    assert_eq!(err.to_string(), "Invalid email or password.");
}

#[tokio::test]
async fn non_json_failure_and_html_success() {
    let mut server = Server::new_async().await;
    server
        .mock("GET", "/api/v1/gallery.php")
        .with_status(503)
        .with_body("down")
        .create_async()
        .await;
    server
        .mock("GET", "/api/v1/streams.php")
        .with_header("content-type", "text/html")
        .with_body("<html>")
        .create_async()
        .await;
    let api = client(&server);
    assert_eq!(
        api.gallery().await.unwrap_err().to_string(),
        "Request failed (503)"
    );
    assert!(matches!(
        api.streams().await.unwrap_err(),
        ApiError::InvalidResponse
    ));
}

#[tokio::test]
async fn staff_requests_send_both_token_headers() {
    let mut server = Server::new_async().await;
    let m = server
        .mock("PATCH", "/api/v1/admin/news.php")
        .match_header("authorization", "Bearer tok")
        .match_header("x-fwg-staff-token", "tok")
        .match_body(Matcher::Json(serde_json::json!({"id":5,"published":false})))
        .with_header("content-type", "application/json")
        .with_body(r#"{"success":true}"#)
        .create_async()
        .await;
    let r = client(&server)
        .update_news("tok", 5, None, None, Some(false))
        .await
        .unwrap();
    assert!(r.success);
    m.assert_async().await;
}

#[tokio::test]
async fn gallery_upload_is_multipart() {
    let mut server = Server::new_async().await;
    let m = server
        .mock("POST", "/api/v1/admin/gallery-upload.php")
        .match_header(
            "content-type",
            Matcher::Regex("^multipart/form-data; boundary=".into()),
        )
        .match_body(Matcher::AllOf(vec![
            Matcher::Regex(r#"name="title"\r\n\r\nCommunity upload"#.into()),
            Matcher::Regex(
                r#"name="image"; filename="upload.jpg"\r\nContent-Type: image/jpeg"#.into(),
            ),
        ]))
        .with_header("content-type", "application/json")
        .with_body(
            r#"{"item":{"id":"9","title":"Community upload","image_url":"https://x/9.jpg"}}"#,
        )
        .create_async()
        .await;
    let image = ImageUpload {
        bytes: vec![0xff, 0xd8],
        file_name: None,
        mime_type: None,
    };
    let r = client(&server)
        .upload_gallery("tok", "  ", image)
        .await
        .unwrap();
    assert_eq!(r.item.id, "9");
    m.assert_async().await;
}

fn login_mock(server: &mut Server, token: &str) -> mockito::Mock {
    server
        .mock("POST", "/api/v1/auth/login.php")
        .with_header("content-type", "application/json")
        .with_body(format!(
            r#"{{"token":"{token}","user":{{"name":"Cobra","email":"c@x","role":"owner"}}}}"#
        ))
        .create()
}

#[tokio::test]
async fn session_persists_and_expires_on_401() {
    let mut server = Server::new_async().await;
    let _login = login_mock(&mut server, "t1");
    server
        .mock("GET", "/api/v1/admin/staff.php")
        .with_status(401)
        .with_header("content-type", "application/json")
        .with_body(r#"{"error":"Session expired"}"#)
        .create_async()
        .await;

    let store: Arc<dyn KeyValueStore> = Arc::new(MemoryStore::default());
    let session = Session::restore(client(&server), store.clone());
    assert!(!session.is_signed_in());

    let user = session.sign_in("  c@x ", "pw").await.unwrap();
    assert_eq!(user.role, "owner");
    assert_eq!(store.get("fwg.staff.token").unwrap().as_deref(), Some("t1"));

    // A fresh app start restores the session from the store.
    let restored = Session::restore(client(&server), store.clone());
    assert_eq!(restored.user().unwrap().name, "Cobra");

    let err = session.api().staff("t1").await.unwrap_err();
    assert!(err.is_unauthorized());
    assert!(!session.is_signed_in());
    assert_eq!(session.take_notice().as_deref(), Some(SESSION_EXPIRED));
    assert_eq!(session.take_notice(), None);
    assert_eq!(store.get("fwg.staff.token").unwrap(), None);
}

#[tokio::test]
async fn late_401_for_replaced_token_is_ignored() {
    let mut server = Server::new_async().await;
    let _login = login_mock(&mut server, "new");
    server
        .mock("GET", "/api/v1/admin/staff.php")
        .with_status(401)
        .create_async()
        .await;

    let store: Arc<dyn KeyValueStore> = Arc::new(MemoryStore::default());
    store.set("fwg.staff.token", "old").unwrap();
    let session = Session::restore(client(&server), store.clone());
    session.sign_in("c@x", "pw").await.unwrap();

    let _ = session.api().staff("old").await;
    assert_eq!(session.token().as_deref(), Some("new"));
    assert!(!session.has_notice());
}

#[tokio::test]
async fn sign_out_clears_even_if_logout_fails() {
    let mut server = Server::new_async().await;
    let _login = login_mock(&mut server, "t");
    server
        .mock("POST", "/api/v1/auth/logout.php")
        .with_status(500)
        .create_async()
        .await;
    let store: Arc<dyn KeyValueStore> = Arc::new(MemoryStore::default());
    let session = Session::restore(client(&server), store.clone());
    session.sign_in("c@x", "pw").await.unwrap();
    session.sign_out().await.unwrap();
    assert!(!session.is_signed_in());
    assert_eq!(store.get("fwg.staff.user").unwrap(), None);
}

#[tokio::test]
async fn network_failure_has_friendly_message() {
    let api = ApiClient::new("http://127.0.0.1:9/api/v1");
    let err = api.news().await.unwrap_err();
    assert!(matches!(err, ApiError::Network(_)));
    let wrapped: Error = err.into();
    assert!(
        wrapped
            .to_string()
            .starts_with("Unable to reach the server")
    );
}

/// Hits the real website's public endpoints. Run with `cargo test -- --ignored live_`.
#[tokio::test]
#[ignore]
async fn live_public_endpoints() {
    let api = ApiClient::default();
    let results = [
        ("news", api.news().await.map(|r| r.items.len())),
        ("streams", api.streams().await.map(|r| r.items.len())),
        ("gallery", api.gallery().await.map(|r| r.items.len())),
        ("games", api.games().await.map(|r| r.items.len())),
        ("servers", api.servers().await.map(|r| r.items.len())),
        ("about", api.about().await.map(|r| r.staff.len())),
    ];
    let mut failed = false;
    for (name, r) in results {
        match r {
            Ok(n) => println!("{name}: ok ({n} items)"),
            Err(e) => {
                failed = true;
                println!("{name}: FAILED {e:?}");
            }
        }
    }
    assert!(!failed);
}
