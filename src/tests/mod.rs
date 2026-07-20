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
