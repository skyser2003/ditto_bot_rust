use std::sync::{Arc, Mutex};

use axum::{
    http::{HeaderMap, StatusCode},
    routing::post,
    Json, Router,
};
use serde_json::{json, Value};

use super::*;
use crate::test::{MockBot, MockMessage};

#[test]
fn command_keeps_input_text_and_requires_the_mod_token() {
    assert_eq!(parse_command("<@BOT> mod hello", "<@BOT>"), Some("hello"));
    assert_eq!(
        parse_command(" \t<@BOT>\tmod\t첫째\n    second <@BOT> ", "<@BOT>"),
        Some("첫째\n    second <@BOT> ")
    );
    assert_eq!(parse_command("<@BOT> mod", "<@BOT>"), Some(""));
    for text in [
        "mod hello",
        "<@OTHER> mod hello",
        "<@BOT> model",
        "<@BOT> moderation",
        "<@BOT> bot mod",
        "<@BOT> bot mod hello",
        "<@BOT> gpt mod hello",
        "hello <@BOT> mod text",
    ] {
        assert_eq!(parse_command(text, "<@BOT>"), None, "command: {:?}", text);
    }
}

struct TestApi {
    url: String,
    requests: Arc<Mutex<Vec<(HeaderMap, Value)>>>,
    task: tokio::task::JoinHandle<()>,
}

impl TestApi {
    fn new(status: StatusCode, body: &str) -> Self {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/v1/moderations", listener.local_addr().unwrap());
        let requests = Arc::new(Mutex::new(vec![]));
        let captured = requests.clone();
        let body = body.to_string();
        let app = Router::new().route(
            "/v1/moderations",
            post(move |headers: HeaderMap, Json(input): Json<Value>| {
                captured.lock().unwrap().push((headers, input));
                let body = body.clone();
                async move { (status, [("content-type", "application/json")], body) }
            }),
        );
        let task = tokio::spawn(async move {
            axum::Server::from_tcp(listener)
                .unwrap()
                .serve(app.into_make_service())
                .await
                .unwrap();
        });
        Self {
            url,
            requests,
            task,
        }
    }

    fn request(&self, input: &str) -> reqwest::RequestBuilder {
        let client = reqwest::Client::builder().no_proxy().build().unwrap();
        request(&client, &self.url, "test-key", input)
    }
}

impl Drop for TestApi {
    fn drop(&mut self) {
        self.task.abort();
    }
}

fn message() -> MessageEvent {
    super::super::test::event("<@> mod sample", vec![])
}

fn reply_text(bot: &MockBot) -> String {
    let messages = bot.dump_messages().unwrap();
    assert_eq!(messages.len(), 1);
    let blocks = match &messages[0].1 {
        MockMessage::Blocks(blocks) => blocks,
        _ => panic!("Expected moderation blocks"),
    };
    let blocks = serde_json::to_value(blocks).unwrap();
    assert_eq!(blocks[0]["text"]["text"], "`Moderation`");
    blocks[1]["text"]["text"].as_str().unwrap().to_string()
}

#[tokio::test]
async fn api_receives_authenticated_text_input_and_returns_only_true_categories() {
    let api = TestApi::new(StatusCode::OK, &json!({
        "id": "modr-test", "model": "omni-moderation-latest",
        "results": [{
            "flagged": true,
            "categories": {"violence": true, "harassment/threatening": true, "hate": false, "future/category": true},
            "category_scores": {"hate": 0.99}
        }]
    }).to_string());
    let input = "first line\n    second <@BOT> & text";
    let bot = MockBot::default();
    handle_request(&bot, &message(), input, api.request(input))
        .await
        .unwrap();
    assert_eq!(
        reply_text(&bot),
        "Flagged categories:\n• future/category\n• harassment/threatening\n• violence"
    );
    let requests = api.requests.lock().unwrap();
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].0["authorization"], "Bearer test-key");
    assert_eq!(
        requests[0].1,
        json!({"model": "omni-moderation-latest", "input": input})
    );
}

#[tokio::test]
async fn no_true_categories_returns_a_clear_result() {
    let api = TestApi::new(
        StatusCode::OK,
        r#"{"results":[{"flagged":false,"categories":{"hate":false,"violence":false},"category_scores":{"hate":0.99}}]}"#,
    );
    let bot = MockBot::default();
    handle_request(&bot, &message(), "sample", api.request("sample"))
        .await
        .unwrap();
    assert_eq!(reply_text(&bot), "No flagged categories.");
}

#[tokio::test]
async fn flags_are_combined_without_duplicates() {
    let api = TestApi::new(
        StatusCode::OK,
        r#"{"results":[{"categories":{"hate":true,"violence":false}},{"categories":{"hate":true,"violence":true}}]}"#,
    );
    assert_eq!(
        flagged_categories(api.request("sample"))
            .await
            .unwrap()
            .into_iter()
            .collect::<Vec<_>>(),
        vec!["hate", "violence"]
    );
}

#[tokio::test]
async fn whitespace_only_input_returns_usage_without_an_api_call() {
    let api = TestApi::new(
        StatusCode::OK,
        r#"{"results":[{"categories":{"hate":true}}]}"#,
    );
    let bot = MockBot::default();
    handle_request(&bot, &message(), " \n\t", api.request(" \n\t"))
        .await
        .unwrap();
    assert!(reply_text(&bot).starts_with("Usage:"));
    assert!(api.requests.lock().unwrap().is_empty());
}

#[tokio::test]
async fn failed_or_malformed_api_responses_are_reported_as_errors() {
    for (status, body) in [
        (
            StatusCode::UNAUTHORIZED,
            r#"{"error":{"message":"private input"}}"#,
        ),
        (
            StatusCode::TOO_MANY_REQUESTS,
            r#"{"error":{"message":"private input"}}"#,
        ),
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            r#"{"results":[{"categories":{"hate":true}}]}"#,
        ),
        (StatusCode::OK, "not json"),
        (StatusCode::OK, r#"{"results":[]}"#),
        (StatusCode::OK, r#"{"results":[{"categories":{}}]}"#),
        (
            StatusCode::OK,
            r#"{"results":[{"categories":{"hate":"true"}}]}"#,
        ),
    ] {
        let api = TestApi::new(status, body);
        let bot = MockBot::default();
        handle_request(&bot, &message(), "sample", api.request("sample"))
            .await
            .unwrap();
        assert_eq!(
            reply_text(&bot),
            "OpenAI moderation request failed. Please try again.",
            "status: {} body: {}",
            status,
            body
        );
    }
}
