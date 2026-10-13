use std::convert::TryFrom;

use crate::{slack, MessageEvent};

struct MessageEventFixture;

impl MessageEventFixture {
    fn parse(thread_ts_field: &str, subtype_field: &str) -> MessageEvent {
        let json = format!(
            r#"{{
                "type": "message",
                "channel": "C123",
                "user": "U123",
                "text": "<@BOT> gpt hello",
                "ts": "1673464745.620769",
                "event_ts": "1673464745.620769",
                "blocks": []
                {thread_ts_field}
                {subtype_field}
            }}"#
        );
        let event = serde_json::from_str::<slack::InternalEvent>(&json)
            .expect("Slack message event should deserialize");

        MessageEvent::try_from(&event).expect("Slack message should convert to MessageEvent")
    }
}

#[test]
fn thread_broadcast_subtype_enables_reply_broadcast() {
    let message = MessageEventFixture::parse(
        r#", "thread_ts": "1673464730.703009""#,
        r#", "subtype": "thread_broadcast""#,
    );

    assert!(message.reply_broadcast);
}

#[test]
fn regular_thread_reply_keeps_reply_in_thread() {
    let message = MessageEventFixture::parse(r#", "thread_ts": "1673464730.703009""#, "");

    assert!(!message.reply_broadcast);
}

#[test]
fn first_message_always_enables_reply_broadcast() {
    let message = MessageEventFixture::parse("", "");

    assert!(message.reply_broadcast);
}

#[test]
fn image_file_share_without_blocks_is_accepted() {
    let event = serde_json::from_value::<slack::InternalEvent>(serde_json::json!({
        "type": "message",
        "subtype": "file_share",
        "channel": "C123",
        "user": "U123",
        "text": "<@BOT> gpt what is in this image?",
        "ts": "1673464745.620769",
        "event_ts": "1673464745.620769",
        "files": [{
            "id": "F123",
            "mimetype": "image/png",
            "size": 68,
            "url_private": "https://files.slack.com/files-pri/T123-F123/image.png"
        }]
    }))
    .unwrap();

    let message = MessageEvent::try_from(&event).unwrap();
    assert_eq!(message.files.len(), 1);
    assert_eq!(message.files[0].id, "F123");
    assert_eq!(message.files[0].mimetype, "image/png");
}
