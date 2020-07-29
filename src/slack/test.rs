use super::*;

#[test]
pub fn test_deserialize_basic_message() {
    serde_json::from_str::<Message>(
        r#"{
        "type": "message",
        "channel": "C2147483705",
        "user": "U2147483697",
        "text": "Hello world",
        "ts": "1355517523.000005"
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
        "ts": "1355517523.000005"
    }"#,
    )
    .unwrap();
    if let Message::BasicMessage(msg) = deserialized {
        assert_eq!(msg.common.text.as_ref(), "그아아");
    } else {
        panic!("deserialized one must be a BasicMessage!");
    }
}

#[test]
pub fn test_deserialize_bot_message() {
    serde_json::from_str::<Message>(
        r#"{
        "type": "message",
        "subtype": "bot_message",
        "ts": "1358877455.000010",
        "text": "Pushing is the answer",
        "bot_id": "BB12033",
        "username": "github",
        "icons": {}
    }"#,
    )
    .unwrap();
}
