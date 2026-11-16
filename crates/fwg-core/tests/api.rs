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
async fn runescape_board_and_signup() {
    use fwg_core::models::{RsGame, RsPeriod, RsSignup};
    let mut server = Server::new_async().await;
    // Shape copied from the website's real response.
    let board = server
        .mock("GET", "/api/v1/runescape.php")
        .match_query(Matcher::AllOf(vec![
            Matcher::UrlEncoded("game".into(), "osrs".into()),
            Matcher::UrlEncoded("period".into(), "week".into()),
            Matcher::UrlEncoded("metric".into(), "overall".into()),
        ]))
        .with_header("content-type", "application/json")
        .with_body(
            r#"{"game":"osrs","period":"week","games":{"rs3":{"label":"RuneScape 3","short":"RS3"}},
            "periods":{"week":{"label":"This week"}},"updated_at":"2026-11-16T19:19:17Z","items":[
            {"name":"Le Me","total_level":3232,"total_xp":5800000000,"rank":34,"updated_at":"2026-11-16T19:19:17Z",
             "gained":1000000,"since":"2026-11-08T19:19:34Z","partial":false,"top_skill":{"name":"Attack","xp":1000000}},
            {"name":"New One","total_level":40,"total_xp":null,"rank":null,"updated_at":null,
             "gained":0,"since":null,"partial":true,"top_skill":null}]}"#,
        )
        .create_async()
        .await;
    let api = client(&server);
    let b = api
        .runescape(RsGame::Osrs, RsPeriod::Week, "overall")
        .await
        .unwrap();
    assert_eq!(
        (b.game, b.period, b.items.len()),
        (RsGame::Osrs, RsPeriod::Week, 2)
    );
    assert_eq!(b.items[0].top_skill.as_ref().unwrap().name, "Attack");
    assert_eq!(b.items[1].total_xp, None);
    assert!(b.items[1].partial);
    board.assert_async().await;

    let signup = server
        .mock("POST", "/api/v1/runescape/signup.php")
        .match_body(Matcher::Json(serde_json::json!({
            "discord":"alice","rs3_name":"","osrs_name":"Lynx Titan","message":"","consent":true
        })))
        .with_status(201)
        .with_header("content-type", "application/json")
        .with_body(r#"{"success":true,"message":"Thanks!","warnings":["We couldn't find it."]}"#)
        .create_async()
        .await;
    let result = api
        .runescape_signup(&RsSignup {
            discord: "alice".into(),
            osrs_name: "Lynx Titan".into(),
            consent: true,
            ..Default::default()
        })
        .await
        .unwrap();
    assert!(result.success);
    assert_eq!(result.warnings.len(), 1);
    signup.assert_async().await;
}

#[tokio::test]
async fn runescape_boards_profile_feed_and_competitions() {
    use fwg_core::models::{RsCompetitionStatus, RsGame, RsPeriod};
    let mut server = Server::new_async().await;
    let api = client(&server);
    // Shapes copied from the website's real responses.
    let board = server
        .mock("GET", "/api/v1/runescape.php")
        .match_query(Matcher::AllOf(vec![
            Matcher::UrlEncoded("game".into(), "osrs".into()),
            Matcher::UrlEncoded("metric".into(), "activity:Clue Scrolls (all)".into()),
        ]))
        .with_header("content-type", "application/json")
        .with_body(
            r#"{"game":"osrs","period":"all","metric":"activity:Clue Scrolls (all)","metric_label":"Clue Scrolls (all)",
            "unit":"score","metrics":[{"value":"overall","label":"Overall","group":"Overall"},
            {"value":"activity:Clue Scrolls (all)","label":"Clue Scrolls (all)","group":"Bosses and activities"}],
            "updated_at":"2026-11-16T20:21:56Z","items":[{"id":4,"name":"Settled","total_level":2322,"total_xp":442439728,
            "rank":12,"value":333,"level":null,"unit":"score","updated_at":"2026-11-16T20:21:56Z","gained":null,"since":null,
            "partial":false,"top_skill":null,"account_type":"ultimate","account_label":"Ultimate Ironman","account_short":"UIM"}]}"#,
        )
        .create_async()
        .await;
    let b = api
        .runescape(RsGame::Osrs, RsPeriod::All, "activity:Clue Scrolls (all)")
        .await
        .unwrap();
    assert_eq!((b.unit.as_str(), b.metrics.len()), ("score", 2));
    assert_eq!(
        (
            b.items[0].id,
            b.items[0].value,
            b.items[0].account_short.as_str()
        ),
        (4, Some(333), "UIM")
    );
    board.assert_async().await;

    server
        .mock("GET", "/api/v1/runescape/player.php?id=2")
        .with_header("content-type", "application/json")
        .with_body(
            r#"{"id":2,"name":"Le Me","games":[{"game":"rs3","label":"RuneScape 3","name":"Le Me","updated_at":"2026-11-16T20:22:15Z",
            "account_type":null,"account_label":"Regular account","account_short":"","death":null,"total_level":3232,
            "total_xp":5800000000,"rank":34,"week_gained":0,"week_since":"2026-11-16T20:17:01Z",
            "skills":[{"name":"Attack","level":120,"xp":200000000,"rank":12,"week_gained":0}],
            "activities":[{"name":"Bounty Hunters","score":275,"rank":3000,"week_gained":0}],
            "history":[{"date":"2026-11-16","total_xp":5800000000,"total_level":3232}],
            "quests":{"qp":476,"qp_max":476,"completed":360,"started":1,"total":366,"quests":[]}},
            {"game":"osrs","label":"Old School","name":"Fallen","account_type":"hardcore_fallen","account_label":"Fallen Hardcore",
            "account_short":"HC†","death":{"text":"lost Hardcore Ironman status","detail":"Lumbridge","at":null},"updated_at":null}],
            "events":[{"id":9,"member_id":2,"name":"Le Me","game":"rs3","kind":"log","text":"found a dragon helm",
            "detail":"After killing a Abyssal lord, it dropped a dragon helm.","happened_at":"2026-11-11T16:52:43Z"}]}"#,
        )
        .create_async()
        .await;
    let p = api.runescape_player(2).await.unwrap();
    assert_eq!(p.games[0].quests.as_ref().unwrap().completed, 360);
    assert_eq!(p.games[0].skills[0].level, Some(120));
    assert_eq!(
        p.games[1].death.as_ref().unwrap().detail.as_deref(),
        Some("Lumbridge")
    );
    assert_eq!(p.games[1].total_xp, None);
    assert_eq!(p.events[0].kind, "log");

    server
        .mock("GET", "/api/v1/runescape/feed.php?limit=10")
        .with_header("content-type", "application/json")
        .with_body(r#"{"items":[{"id":1,"member_id":4,"name":"Settled","game":"osrs","kind":"milestone","text":"reached 2,500 Zulrah","detail":null,"happened_at":"2026-11-16T20:21:56Z"}]}"#)
        .create_async()
        .await;
    assert_eq!(
        api.runescape_feed(10).await.unwrap().items[0].game,
        RsGame::Osrs
    );

    server
        .mock("GET", "/api/v1/runescape/competitions.php")
        .with_header("content-type", "application/json")
        .with_body(r#"{"items":[{"id":2,"title":"Boss of the Week: Zulrah","game":"osrs","game_label":"Old School","metric":"activity:Zulrah","metric_label":"Zulrah","unit":"score","starts_at":"2026-11-16T21:22:21Z","ends_at":"2026-11-23T20:22:21Z","status":"upcoming"}]}"#)
        .create_async()
        .await;
    assert_eq!(
        api.runescape_competitions().await.unwrap().items[0].status,
        RsCompetitionStatus::Upcoming
    );

    server
        .mock("GET", "/api/v1/runescape/competitions.php?id=1")
        .with_header("content-type", "application/json")
        .with_body(r#"{"id":1,"title":"Test SOTW: Attack","game":"osrs","game_label":"Old School","metric":"skill:Attack","metric_label":"Attack","unit":"xp","starts_at":"2026-11-16T20:21:22Z","ends_at":"2026-11-16T20:21:22Z","status":"ended","items":[{"id":4,"name":"Settled","gained":123456,"start":1,"end":123457,"level":99,"updated_at":"2026-11-16T20:22:22Z","account_type":"ultimate","account_label":"Ultimate Ironman","account_short":"UIM"},{"id":5,"name":"Late","gained":null,"start":null,"end":null,"level":null,"updated_at":null,"account_type":null,"account_label":"Regular account","account_short":""}]}"#)
        .create_async()
        .await;
    let c = api.runescape_competition(1).await.unwrap();
    assert_eq!(c.competition.status, RsCompetitionStatus::Ended);
    assert_eq!((c.items[0].gained, c.items[1].gained), (Some(123456), None));

    server
        .mock("GET", "/api/v1/runescape/player.php?id=999")
        .with_status(404)
        .with_header("content-type", "application/json")
        .with_body(r#"{"error":"Player not found."}"#)
        .create_async()
        .await;
    assert!(
        api.runescape_player(999)
            .await
            .unwrap_err()
            .to_string()
            .contains("Player not found")
    );
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
