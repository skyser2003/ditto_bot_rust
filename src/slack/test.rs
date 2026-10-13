use super::*;

#[test]
pub fn test_deserialize_basic_message() {
    serde_json::from_str::<Message>(
        r#"{
        "type": "message",
        "channel": "C2147483705",
        "user": "U2147483697",
        "text": "Hello world",
        "ts": "1355517523.000005",
        "event_ts": "1355517523.000005",
        "blocks": []
    }"#,
    )
    .unwrap();
}

#[test]
pub fn test_deserialize_unicode_basic_message() {
    let deserialized = serde_json::from_str::<Message>(
        r#"{
        "type": "message",
        "channel": "C2147483705",
        "user": "U2147483697",
        "text": "\uadf8\uc544\uc544",
        "ts": "1355517523.000005",
        "event_ts": "1355517523.000005",
        "blocks": []
    }"#,
    )
    .unwrap();
    if let Message::BasicMessage(msg) = deserialized {
        assert_eq!(&msg.common.text, "그아아");
    } else {
        panic!("deserialized one must be a BasicMessage!");
    }
}

#[test]
pub fn test_serde_enum() {
    assert_eq!(
        serde_json::from_str::<TextObjectType>("\"plain_text\"").unwrap(),
        TextObjectType::PlainText
    );
    assert_eq!(
        serde_json::from_str::<TextObjectType>("\"mrkdwn\"").unwrap(),
        TextObjectType::Markdown
    );

    assert_eq!(
        serde_json::to_string(&TextObjectType::PlainText).unwrap(),
        "\"plain_text\""
    );
    assert_eq!(
        serde_json::to_string(&TextObjectType::Markdown).unwrap(),
        "\"mrkdwn\""
    );
}

#[test]
pub fn test_deserialize_bot_message() {
    serde_json::from_str::<Message>(
        r#"{
        "type": "message",
        "channel": "ASDF1234",
        "subtype": "bot_message",
        "ts": "1358877455.000010",
        "event_ts": "1358877455.000010",
        "blocks": [],
        "text": "Pushing is the answer",
        "bot_id": "BB12033",
        "username": "github",
        "icons": {}
    }"#,
    )
    .unwrap();
}

#[test]
pub fn test_deserialize_normal_message() {
    serde_json::from_str::<Message>(
        r#"{
        "type": "message",
        "ts": "1358877455.000010",
        "channel": "aaaa",
        "text": "Pushing is the answer",
        "event_ts": "1358877455.000010",
        "blocks": [],
        "user": "github"
    }"#,
    )
    .unwrap();
}

#[test]
fn image_only_thread_message_is_preserved() {
    let message = serde_json::from_value::<ThreadMessageType>(serde_json::json!({
        "type": "message",
        "subtype": "file_share",
        "user": "U123",
        "text": "",
        "ts": "1673464745.620769",
        "files": [{
            "id": "F123",
            "mimetype": "image/png",
            "url_private": "https://files.slack.com/files-pri/T123-F123/image.png"
        }]
    }))
    .unwrap();

    if let ThreadMessageType::Unbroadcasted(message) = message {
        assert!(message.text.is_empty());
        assert_eq!(message.files.len(), 1);
        assert_eq!(message.files[0].id, "F123");
    } else {
        panic!("image-only thread message must preserve its files");
    }
}
