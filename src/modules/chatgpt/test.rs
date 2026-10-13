use std::convert::TryFrom;

use serde_json::{json, Value};

use super::*;
use crate::{slack, test::MockBot, MessageEvent};

const PNG_URL: &str = "https://files.slack.com/files-pri/T123-F123/image.png";
const PNG_DATA: &str = "data:image/png;base64,aW1hZ2U=";

fn file(id: &str, mimetype: &str, url: &str) -> Value {
    json!({ "id": id, "mimetype": mimetype, "size": 5, "url_private": url })
}

fn event(text: &str, files: Vec<Value>) -> MessageEvent {
    let event = serde_json::from_value::<slack::InternalEvent>(json!({
        "type": "message", "user": "U123", "channel": "C123", "text": text,
        "ts": "1673464745.620769", "event_ts": "1673464745.620769", "files": files
    }))
    .unwrap();
    MessageEvent::try_from(&event).unwrap()
}

fn bot() -> MockBot {
    let bot = MockBot::default();
    bot.files
        .write()
        .unwrap()
        .insert("F123".to_string(), bytes::Bytes::from_static(b"image"));
    bot
}

async fn input(bot: &MockBot, msg: &MessageEvent, messages: Vec<Value>) -> Value {
    let messages = messages
        .into_iter()
        .map(|value| serde_json::from_value(value).unwrap())
        .collect::<Vec<ThreadMessageType>>();
    let input_text = msg.text.strip_prefix("<@BOT> gpt ").unwrap_or("");
    serde_json::to_value(
        build_responses_input(bot, msg, input_text, "<@BOT> gpt ", &messages)
            .await
            .unwrap(),
    )
    .unwrap()
}

#[tokio::test]
async fn attached_image_is_encoded_as_openai_input() {
    let msg = event(
        "<@BOT> gpt describe this",
        vec![file("F123", "image/png", PNG_URL)],
    );
    assert_eq!(
        input(&bot(), &msg, vec![]).await,
        json!([{"role": "user", "content": [
            {"type": "input_text", "text": "describe this"},
            {"type": "input_image", "image_url": PNG_DATA, "detail": "auto"}
        ]}])
    );
}

#[tokio::test]
async fn text_only_requests_keep_string_content() {
    let msg = event("<@BOT> gpt hello", vec![]);
    assert_eq!(
        input(&bot(), &msg, vec![]).await,
        json!([{"role": "user", "content": "hello"}])
    );
}

#[tokio::test]
async fn image_only_request_has_image_content() {
    let msg = event("<@BOT> gpt", vec![file("F123", "image/png", PNG_URL)]);
    assert_eq!(
        input(&bot(), &msg, vec![]).await,
        json!([{"role": "user", "content": [
            {"type": "input_image", "image_url": PNG_DATA, "detail": "auto"}
        ]}])
    );
}

#[tokio::test]
async fn current_image_is_included_once_when_already_in_history() {
    let msg = event(
        "<@BOT> gpt describe this",
        vec![file("F123", "image/png", PNG_URL)],
    );
    let result = input(
        &bot(),
        &msg,
        vec![json!({
            "type": "message", "user": "U123", "text": msg.text, "ts": msg.ts,
            "files": [file("F123", "image/png", PNG_URL)]
        })],
    )
    .await;
    assert_eq!(result.as_array().unwrap().len(), 1);
    assert_eq!(result[0]["content"][1]["image_url"], PNG_DATA);
}

#[tokio::test]
async fn current_event_files_are_used_when_history_lacks_files() {
    let msg = event(
        "<@BOT> gpt describe this",
        vec![file("F123", "image/png", PNG_URL)],
    );
    let result = input(
        &bot(),
        &msg,
        vec![json!({
            "type": "message", "user": "U123", "text": msg.text, "ts": msg.ts
        })],
    )
    .await;
    assert_eq!(result.as_array().unwrap().len(), 1);
    assert_eq!(result[0]["content"][1]["image_url"], PNG_DATA);
}

#[tokio::test]
async fn previous_images_and_current_request_are_preserved() {
    let msg = event("<@BOT> gpt compare", vec![]);
    let result = input(
        &bot(),
        &msg,
        vec![json!({
            "type": "message", "user": "U123", "text": "", "ts": "1673464730.703009",
            "files": [file("F123", "image/png", PNG_URL)]
        })],
    )
    .await;
    assert_eq!(
        result,
        json!([
            {"role": "user", "content": [{"type": "input_image", "image_url": PNG_DATA, "detail": "auto"}]},
            {"role": "user", "content": "compare"}
        ])
    );
}

#[tokio::test]
async fn multiple_images_keep_attachment_order_and_skip_non_images() {
    let bot = bot();
    bot.files
        .write()
        .unwrap()
        .insert("F456".to_string(), bytes::Bytes::from_static(b"photo"));
    let msg = event(
        "<@BOT> gpt compare",
        vec![
            file("F123", "image/png", PNG_URL),
            file(
                "F789",
                "application/pdf",
                "https://files.slack.com/document.pdf",
            ),
            file("F456", "image/jpeg", "https://files.slack.com/photo.jpg"),
        ],
    );
    let result = input(&bot, &msg, vec![]).await;
    assert_eq!(
        result[0]["content"],
        json!([
            {"type": "input_text", "text": "compare"},
            {"type": "input_image", "image_url": PNG_DATA, "detail": "auto"},
            {"type": "input_image", "image_url": "data:image/jpeg;base64,cGhvdG8=", "detail": "auto"}
        ])
    );
}

#[tokio::test]
async fn failed_image_download_returns_an_error() {
    let msg = event(
        "<@BOT> gpt describe",
        vec![file("F404", "image/png", PNG_URL)],
    );
    assert!(
        build_responses_input(&bot(), &msg, "describe", "<@BOT> gpt ", &[])
            .await
            .is_err()
    );
}

#[tokio::test]
async fn previous_assistant_reply_keeps_assistant_role() {
    let msg = event("<@BOT> gpt explain more", vec![]);
    for user in [Value::Null, json!("UBOT")] {
        let result = input(
            &bot(),
            &msg,
            vec![json!({
                "type": "message", "subtype": "bot_message", "bot_id": "BOT", "user": user,
                "text": "fallback", "ts": "1673464730.703009",
                "blocks": [
                    {"type": "section", "text": {"type": "mrkdwn", "text": "`ChatGPT`"}},
                    {"type": "section", "text": {"type": "mrkdwn", "text": "An image of a cat"}}
                ]
            })],
        )
        .await;
        assert_eq!(
            result,
            json!([
                {"role": "assistant", "content": "An image of a cat"},
                {"role": "user", "content": "explain more"}
            ])
        );
    }
}

#[tokio::test]
async fn image_download_failure_is_reported_for_commands_without_prompt_text() {
    for text in ["<@>", "<@> gpt", "<@> describe this"] {
        let bot = bot();
        let msg = event(text, vec![file("F404", "image/png", PNG_URL)]);
        handle(&bot, &msg).await.unwrap();
        let messages = bot.dump_messages().unwrap();
        assert_eq!(messages.len(), 1, "command: {:?}", text);
        match &messages[0].1 {
            crate::test::MockMessage::Text(text) => {
                assert!(text.contains("Unable to read an attached image"));
                assert!(text.contains("F404"));
            }
            _ => panic!("Expected an image error message"),
        }
    }
}

#[tokio::test]
async fn image_attachment_requires_bot_mention() {
    let bot = bot();
    let msg = event("describe this", vec![file("F404", "image/png", PNG_URL)]);
    handle(&bot, &msg).await.unwrap();
    assert!(bot.dump_messages().unwrap().is_empty());
}

#[tokio::test]
async fn unsupported_image_format_is_reported() {
    let msg = event(
        "<@BOT> gpt describe",
        vec![file("F123", "image/svg+xml", PNG_URL)],
    );
    let error = build_responses_input(&bot(), &msg, "describe", "<@BOT> gpt ", &[])
        .await
        .unwrap_err();
    assert!(error.to_string().contains("unsupported format"));
}

#[tokio::test]
async fn multiline_current_request_keeps_newlines_and_indentation() {
    let prompt = "Review this code:\n```rust\nfn main() {\n    println!(\"hello\");\n}\n```";
    for (command, gpt_prefix_exists) in [
        (format!(" gpt {}", prompt), true),
        (format!(" {}", prompt), false),
    ] {
        let parsed_text = parse_input_text(&command, gpt_prefix_exists);
        assert_eq!(parsed_text, prompt);
        for attachments in [vec![], vec![file("F123", "image/png", PNG_URL)]] {
            let msg = event(&format!("<@BOT>{}", command), attachments);
            let messages = vec![serde_json::from_value(json!({
                "type": "message", "user": "U123", "text": msg.text, "ts": msg.ts
            }))
            .unwrap()];
            let input = build_responses_input(&bot(), &msg, &parsed_text, "<@BOT> gpt ", &messages)
                .await
                .unwrap();
            let input = serde_json::to_value(input).unwrap();
            let actual_text = if msg.files.is_empty() {
                &input[0]["content"]
            } else {
                &input[0]["content"][0]["text"]
            };
            assert_eq!(actual_text, prompt);
        }
    }
}

#[test]
fn command_followed_by_newline_keeps_prompt_indentation() {
    assert_eq!(parse_input_text(" gpt\n    code", true), "\n    code");
    assert_eq!(parse_input_text("\n    code", false), "\n    code");
    assert_eq!(parse_input_text(" \n    code", false), "\n    code");
}

#[tokio::test]
async fn previous_request_keeps_leading_prompt_whitespace() {
    let msg = event("<@BOT> gpt explain", vec![]);
    let result = input(
        &bot(),
        &msg,
        vec![json!({
            "type": "message", "user": "U123", "text": "<@BOT> gpt \n    code",
            "ts": "1673464730.703009"
        })],
    )
    .await;
    assert_eq!(result[0]["content"], "\n    code");
}

fn rendered_blocks(message: &crate::test::MockMessage) -> Value {
    match message {
        crate::test::MockMessage::Blocks(blocks) => serde_json::to_value(blocks).unwrap(),
        crate::test::MockMessage::BlocksWithText { blocks, .. } => {
            serde_json::to_value(blocks).unwrap()
        }
        _ => panic!("Expected a block message"),
    }
}

#[tokio::test]
async fn completed_reply_uses_slack_rich_text() {
    let bot = bot();
    GptMessageManager::send_message_static(&bot, "**Bold** and `code`", "C123", &None)
        .await
        .unwrap();
    let messages = bot.dump_messages().unwrap();
    match &messages[0].1 {
        crate::test::MockMessage::BlocksWithText { text, .. } => {
            assert_eq!(text, "**Bold** and `code`")
        }
        _ => panic!("Expected original Markdown as the text fallback"),
    }
    let blocks = rendered_blocks(&messages[0].1);
    assert_eq!(blocks[0]["text"]["text"], "`ChatGPT`");
    assert_eq!(blocks[1]["type"], "rich_text");
    assert_eq!(
        blocks[1]["elements"][0]["elements"][0],
        json!({
            "type": "text", "text": "Bold", "style": {"bold": true}
        })
    );
}

#[tokio::test]
async fn streaming_edit_uses_slack_rich_text() {
    let bot = bot();
    let channel = "C123".to_string();
    let manager = GptMessageManager::new(&channel, None);
    manager
        .edit_message(
            &bot,
            "[Link](https://example.com)",
            &channel,
            "1673464745.620769",
            None,
        )
        .await
        .unwrap();
    let messages = bot.dump_messages().unwrap();
    match &messages[0].1 {
        crate::test::MockMessage::BlocksWithText { text, .. } => {
            assert_eq!(text, "[Link](https://example.com)")
        }
        _ => panic!("Expected original Markdown as the text fallback"),
    }
    let blocks = rendered_blocks(&messages[0].1);
    assert_eq!(blocks[1]["type"], "rich_text");
    assert_eq!(
        blocks[1]["elements"][0]["elements"][0],
        json!({
            "type": "link", "url": "https://example.com", "text": "Link"
        })
    );
}

#[tokio::test]
async fn rich_text_assistant_reply_preserves_markdown_in_history() {
    let msg = event("<@BOT> gpt explain more", vec![]);
    let result = input(
        &bot(),
        &msg,
        vec![json!({
            "type": "message", "subtype": "bot_message", "bot_id": "BOT",
            "text": "**An image** of a cat", "ts": "1673464730.703009",
            "blocks": [
                {"type": "section", "text": {"type": "mrkdwn", "text": "`ChatGPT`"}},
                {"type": "rich_text", "elements": [{"type": "rich_text_section", "elements": [
                    {"type": "text", "text": "An image", "style": {"bold": true}},
                    {"type": "text", "text": " of a cat"}
                ]}]}
            ]
        })],
    )
    .await;
    assert_eq!(
        result[0],
        json!({"role": "assistant", "content": "**An image** of a cat"})
    );
}

#[tokio::test]
async fn streaming_status_does_not_break_a_closing_code_fence() {
    for (source, expected_code) in [
        ("```rust\nfn main() {}\n```", "fn main() {}\n"),
        ("```rust\nfn main() {}", "fn main() {}"),
    ] {
        for footer in [" `[continue]`", "`[DONE]`"] {
            let bot = bot();
            let channel = "C123".to_string();
            let mut manager = GptMessageManager::new(&channel, None);
            manager.ts = "1673464745.620769".to_string();
            manager.concat_message(source);
            manager.stream_message(&bot, Some(footer)).await.unwrap();
            assert_eq!(manager.message, source);
            let messages = bot.dump_messages().unwrap();
            let blocks = rendered_blocks(&messages[0].1);
            assert_eq!(blocks[1]["elements"][0]["type"], "rich_text_preformatted");
            assert_eq!(
                blocks[1]["elements"][0]["elements"][0]["text"],
                expected_code
            );
            assert_eq!(blocks[1]["elements"][1]["type"], "rich_text_section");
        }
    }
}

#[test]
fn post_and_edit_payloads_keep_original_markdown_as_text() {
    let source = "**Bold**\n\n```rust\nlet x = 1;\n```";
    let blocks = GptMessageManager::response_blocks(source, None);
    let message = Message::BlocksWithText {
        text: source,
        blocks: &blocks,
    };
    let post = serde_json::to_value(message.as_postmessage("C123", None, None)).unwrap();
    let edit = serde_json::to_value(message.as_editmessage("C123", "1673464745.620769")).unwrap();
    for payload in [post, edit] {
        assert_eq!(payload["text"], source);
        assert_eq!(payload["blocks"][1]["type"], "rich_text");
    }
}
