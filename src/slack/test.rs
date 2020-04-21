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