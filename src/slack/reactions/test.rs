use super::*;
use reqwest::{header, Method, StatusCode};
use serde_json::json;

#[test]
fn reaction_requests_authenticate_and_target_the_input_message() {
    for method in ["add", "remove"] {
        let url = format!("https://slack.com/api/reactions.{}", method);
        let request = reaction_request(
            &reqwest::Client::new(),
            &url,
            "test-token",
            "C123",
            "1673464745.620769",
            "hourglass_flowing_sand",
        )
        .build()
        .unwrap();
        assert_eq!(request.method(), Method::POST);
        assert_eq!(request.url().as_str(), url);
        assert_eq!(
            request.headers()[header::AUTHORIZATION],
            "Bearer test-token"
        );
        assert_eq!(request.headers()[header::CONTENT_TYPE], "application/json");
        assert_eq!(request.timeout(), Some(&Duration::from_secs(5)));
        let body: serde_json::Value =
            serde_json::from_slice(request.body().unwrap().as_bytes().unwrap()).unwrap();
        assert_eq!(
            body,
            json!({
                "channel": "C123",
                "timestamp": "1673464745.620769",
                "name": "hourglass_flowing_sand"
            })
        );
    }
}

fn response(status: StatusCode, body: &str) -> reqwest::Response {
    axum::http::Response::builder()
        .status(status)
        .header(header::CONTENT_TYPE, "application/json")
        .body(reqwest::Body::from(body.to_string()))
        .unwrap()
        .into()
}

#[tokio::test]
async fn reactions_allow_success_and_an_already_applied_action() {
    for ignored_error in ["already_reacted", "no_reaction"] {
        for body in [
            json!({"ok": true}),
            json!({"ok": false, "error": ignored_error}),
        ] {
            read_reaction_response(response(StatusCode::OK, &body.to_string()), ignored_error)
                .await
                .unwrap();
        }
    }
}

#[tokio::test]
async fn failed_or_malformed_reaction_responses_are_rejected() {
    for (status, body) in [
        (StatusCode::OK, r#"{"ok":false,"error":"missing_scope"}"#),
        (StatusCode::OK, r#"{"ok":false,"error":"no_reaction"}"#),
        (StatusCode::OK, r#"{"ok":false}"#),
        (StatusCode::OK, r#"{"error":"already_reacted"}"#),
        (StatusCode::OK, "not json"),
        (StatusCode::TOO_MANY_REQUESTS, r#"{"ok":true}"#),
        (StatusCode::INTERNAL_SERVER_ERROR, r#"{"ok":true}"#),
    ] {
        assert!(
            read_reaction_response(response(status, body), "already_reacted")
                .await
                .is_err(),
            "status: {} body: {}",
            status,
            body
        );
    }
}
