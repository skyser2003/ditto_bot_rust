use axum::{http::StatusCode, routing::post, Router};

use super::*;
use crate::{
    slack::{EditMessageResponse, PostMessageResponse},
    test::MockMessage,
};

struct TestApi {
    url: String,
    task: tokio::task::JoinHandle<()>,
}

impl TestApi {
    fn new(status: StatusCode, content_type: &'static str, body: Vec<u8>) -> Self {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/responses", listener.local_addr().unwrap());
        let app = Router::new().route(
            "/responses",
            post(move || {
                let body = body.clone();
                async move { (status, [("content-type", content_type)], body) }
            }),
        );
        let task = tokio::spawn(async move {
            axum::Server::from_tcp(listener)
                .unwrap()
                .serve(app.into_make_service())
                .await
                .unwrap();
        });
        Self { url, task }
    }
}

impl Drop for TestApi {
    fn drop(&mut self) {
        self.task.abort();
    }
}

fn completed(text: &str) -> Value {
    json!({
        "id": "resp-test", "status": "completed", "output": [{
            "type": "message", "id": "msg-test", "role": "assistant", "status": "completed",
            "content": [{"type": "output_text", "text": text}]
        }]
    })
}

fn events(events: Vec<Value>) -> Vec<u8> {
    events
        .into_iter()
        .map(|event| format!("data: {}\n\n", event))
        .collect::<String>()
        .into_bytes()
}

fn delta(text: &str) -> Value {
    json!({"type": "response.output_text.delta", "item_id": "msg-test", "delta": text})
}

async fn run_api(
    bot: &MockBot,
    msg: &MessageEvent,
    api: &TestApi,
    stream: bool,
) -> anyhow::Result<()> {
    let client = reqwest::Client::builder().no_proxy().build().unwrap();
    let body = OpenAIResponsesBody {
        model: "test-model".to_string(),
        input: vec![],
        temperature: None,
        previous_response_id: None,
        store: true,
        stream,
        tools: vec![],
    };
    let reply = Some(ReplyMessageEvent {
        msg: msg.thread_ts.clone().unwrap_or_else(|| msg.ts.clone()),
        broadcast: msg.reply_broadcast,
    });
    process_request(
        bot,
        msg,
        deliver_response(bot, msg, &client, body, &api.url, reply),
    )
    .await
}

fn assert_generic_error_and_cleanup(bot: &MockBot, msg: &MessageEvent) {
    let messages = bot.dump_messages().unwrap();
    let notifications: Vec<_> = messages
        .iter()
        .filter_map(|(_, message)| match message {
            MockMessage::Text(text) => Some(text.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(notifications, vec![ERROR_MESSAGE]);
    assert!(messages.iter().all(|(channel, _)| channel == &msg.channel));
    assert_processing_reactions(bot, msg);
    let activity = bot.activity.read().unwrap();
    assert_eq!(activity.first(), Some(&"add_reaction"));
    assert_eq!(activity.last(), Some(&"remove_reaction"));
    assert_eq!(activity[activity.len() - 2], "send_message");
    let replies = bot.sent_replies.read().unwrap();
    let reply = replies.last().unwrap().as_ref().unwrap();
    assert_eq!(
        reply.msg,
        msg.thread_ts.as_ref().unwrap_or(&msg.ts).as_str()
    );
    assert_eq!(reply.broadcast, msg.reply_broadcast);
}

#[tokio::test]
async fn success_removes_the_reaction_without_an_error_message() {
    for stream in [false, true] {
        let body = if stream {
            events(vec![
                delta("Hello"),
                delta("."),
                json!({
                    "type": "response.completed", "response": completed("Hello.")
                }),
            ])
        } else {
            completed("Hello.").to_string().into_bytes()
        };
        let content_type = if stream {
            "text/event-stream"
        } else {
            "application/json"
        };
        let api = TestApi::new(StatusCode::OK, content_type, body);
        let bot = bot();
        let msg = event("<@> gpt hello", vec![]);
        run_api(&bot, &msg, &api, stream).await.unwrap();
        assert_processing_reactions(&bot, &msg);
        let messages = bot.dump_messages().unwrap();
        assert!(!messages.is_empty());
        assert!(messages
            .iter()
            .all(|(_, message)| matches!(message, MockMessage::BlocksWithText { .. })));
        let MockMessage::BlocksWithText { text, .. } = &messages.last().unwrap().1 else {
            unreachable!()
        };
        assert!(text.starts_with("Hello."));
        assert_eq!(
            bot.activity.read().unwrap().last(),
            Some(&"remove_reaction")
        );
    }
}

#[tokio::test]
async fn upstream_http_errors_never_expose_the_response_body() {
    for stream in [false, true] {
        for status in [
            StatusCode::BAD_REQUEST,
            StatusCode::UNAUTHORIZED,
            StatusCode::TOO_MANY_REQUESTS,
            StatusCode::INTERNAL_SERVER_ERROR,
        ] {
            let api = TestApi::new(
                status,
                "application/json",
                br#"{"error":{"message":"private diagnostic information"}}"#.to_vec(),
            );
            let bot = bot();
            let mut msg = event("<@> gpt hello", vec![]);
            msg.thread_ts = Some("1673464700.000001".to_string());
            msg.reply_broadcast = true;
            run_api(&bot, &msg, &api, stream).await.unwrap_err();
            assert_generic_error_and_cleanup(&bot, &msg);
        }
    }
}

#[tokio::test]
async fn malformed_failed_and_empty_json_responses_report_a_generic_error() {
    let mut empty_content = completed("Hello");
    empty_content["output"][0]["content"] = json!([]);
    let mut incomplete = completed("Partial answer");
    incomplete["status"] = json!("incomplete");
    for body in [
        b"not json: private diagnostic".to_vec(),
        vec![0xff, 0xfe],
        br#"{"error":{"message":"private diagnostic"}}"#.to_vec(),
        br#"{"id":"resp-test","status":"completed","output":[]}"#.to_vec(),
        empty_content.to_string().into_bytes(),
        incomplete.to_string().into_bytes(),
    ] {
        let api = TestApi::new(StatusCode::OK, "application/json", body);
        let bot = bot();
        let msg = event("<@> gpt hello", vec![]);
        run_api(&bot, &msg, &api, false).await.unwrap_err();
        assert_generic_error_and_cleanup(&bot, &msg);
    }
}

#[tokio::test]
async fn interrupted_failed_and_malformed_streams_report_a_generic_error() {
    for body in [
        vec![],
        b"data: not json: private diagnostic\n\n".to_vec(),
        events(vec![delta("Partial answer")]),
        events(vec![
            json!({"type": "response.failed", "response": {"error": "private diagnostic"}}),
        ]),
        events(vec![json!({"type": "response.incomplete"})]),
        events(vec![
            json!({"type": "error", "message": "private diagnostic"}),
        ]),
        events(vec![
            json!({"type": "response.completed", "response": completed("")}),
        ]),
    ] {
        let api = TestApi::new(StatusCode::OK, "text/event-stream", body);
        let bot = bot();
        let msg = event("<@> gpt hello", vec![]);
        run_api(&bot, &msg, &api, true).await.unwrap_err();
        assert_generic_error_and_cleanup(&bot, &msg);
    }
}

#[tokio::test]
async fn tool_failures_report_a_generic_error_in_both_modes() {
    for stream in [false, true] {
        let response = json!({"id": "resp-test", "status": "completed", "output": [{
            "type": "function_call", "id": "fc-test", "status": "completed",
            "call_id": "call-test", "name": "test-tool", "arguments": "{}"
        }]});
        let (content_type, body) = if stream {
            (
                "text/event-stream",
                events(vec![
                    json!({"type": "response.completed", "response": response}),
                ]),
            )
        } else {
            ("application/json", response.to_string().into_bytes())
        };
        let api = TestApi::new(StatusCode::OK, content_type, body);
        let bot = bot();
        let msg = event("<@> gpt hello", vec![]);
        run_api(&bot, &msg, &api, stream).await.unwrap_err();
        assert_generic_error_and_cleanup(&bot, &msg);
    }
}

#[tokio::test]
async fn rejected_or_failed_slack_posts_report_a_generic_error() {
    for stream in [false, true] {
        for result in [
            Err(anyhow!("private transport diagnostic")),
            Ok(PostMessageResponse {
                ok: false,
                channel: None,
                ts: None,
                error: Some("private rejection".to_string()),
            }),
            Ok(PostMessageResponse {
                ok: true,
                channel: None,
                ts: None,
                error: None,
            }),
        ] {
            // A timestamp is required when streaming because later updates edit the message.
            if !stream && result.as_ref().map(|result| result.ok).unwrap_or(false) {
                continue;
            }
            let response = completed("Hello.");
            let (content_type, body) = if stream {
                (
                    "text/event-stream",
                    events(vec![
                        json!({"type": "response.completed", "response": response}),
                    ]),
                )
            } else {
                ("application/json", response.to_string().into_bytes())
            };
            let api = TestApi::new(StatusCode::OK, content_type, body);
            let bot = bot();
            bot.post_results.write().unwrap().push_back(result);
            let msg = event("<@> gpt hello", vec![]);
            run_api(&bot, &msg, &api, stream).await.unwrap_err();
            assert_generic_error_and_cleanup(&bot, &msg);
        }
    }
}

#[tokio::test]
async fn failed_slack_stream_updates_report_a_generic_error() {
    for result in [
        Err(anyhow!("private update diagnostic")),
        Ok(EditMessageResponse {
            ok: false,
            channel: None,
            ts: None,
            error: Some("private rejection".to_string()),
        }),
    ] {
        let api = TestApi::new(
            StatusCode::OK,
            "text/event-stream",
            events(vec![
                delta("Hello"),
                json!({
                    "type": "response.completed", "response": completed("Hello")
                }),
            ]),
        );
        let bot = bot();
        bot.edit_results.write().unwrap().push_back(result);
        let msg = event("<@> gpt hello", vec![]);
        run_api(&bot, &msg, &api, true).await.unwrap_err();
        assert_generic_error_and_cleanup(&bot, &msg);
    }
}

#[tokio::test]
async fn cleanup_runs_even_if_the_error_notification_cannot_be_sent() {
    for result in [
        Err(anyhow!("private Slack diagnostic")),
        Ok(PostMessageResponse {
            ok: false,
            channel: None,
            ts: None,
            error: Some("private rejection".to_string()),
        }),
    ] {
        let bot = bot();
        bot.post_results.write().unwrap().push_back(result);
        let msg = event("<@> gpt hello", vec![]);
        let error = process_request(&bot, &msg, async { bail!("original request failure") })
            .await
            .unwrap_err();
        assert_eq!(error.to_string(), "original request failure");
        assert_generic_error_and_cleanup(&bot, &msg);
    }
}

#[tokio::test]
async fn unexpected_panics_notify_the_user_and_run_cleanup() {
    let bot = bot();
    let msg = event("<@> gpt hello", vec![]);
    process_request(&bot, &msg, async { panic!("private panic diagnostic") })
        .await
        .unwrap_err();
    assert_generic_error_and_cleanup(&bot, &msg);
}
