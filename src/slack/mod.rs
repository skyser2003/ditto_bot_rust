use std::{
    fmt::{Debug, Formatter},
    time::{Duration, SystemTime},
};

use serde::{Deserialize, Serialize};

#[derive(Clone, Deserialize, Serialize, PartialEq, Eq)]
pub struct StrTimeStamp(String);

impl From<&StrTimeStamp> for SystemTime {
    fn from(val: &StrTimeStamp) -> SystemTime {
        SystemTime::UNIX_EPOCH + Duration::from_secs_f32(val.0.parse().unwrap())
    }
}

impl From<&StrTimeStamp> for String {
    fn from(val: &StrTimeStamp) -> Self {
        val.0.clone()
    }
}

impl Debug for StrTimeStamp {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        let t: SystemTime = self.into();
        write!(f, "{}({:?})", self.0, t)
    }
}

#[derive(Clone, Deserialize, Serialize, PartialEq, Eq)]
pub struct NumericTimeStamp(u64);

impl From<&NumericTimeStamp> for SystemTime {
    fn from(val: &NumericTimeStamp) -> SystemTime {
        SystemTime::UNIX_EPOCH + Duration::from_secs(val.0)
    }
}

impl Debug for NumericTimeStamp {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        let t: SystemTime = self.into();
        write!(f, "{}({:?})", self.0, t)
    }
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub enum TextObjectType {
    #[serde(rename = "plain_text")]
    PlainText,
    #[serde(rename = "mrkdwn")]
    Markdown,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct TextObject {
    #[serde(rename = "type")]
    pub ty: TextObjectType,
    pub text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub emoji: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub verbatim: Option<bool>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct SectionBlock {
    pub text: TextObject,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub block_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fields: Option<Vec<TextObject>>, //pub accessory:
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ActionBlock {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub block_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub elements: Option<Vec<BlockElement>>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub enum ButtonStyle {
    #[serde(rename = "primary")]
    Primary,
    #[serde(rename = "danger")]
    Danger,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ButtonBlock {
    pub text: TextObject,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub action_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub style: Option<ButtonStyle>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImageBlock {
    #[serde(rename = "type")]
    pub ty: String,
    pub image_url: String,
    pub alt_text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<TextObject>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub block_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LinkBlock {
    pub url: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(tag = "type")]
#[serde(rename_all = "snake_case")]
pub enum BlockElement {
    RichText {
        block_id: String,
        elements: Vec<BlockElement>,
    },
    RichTextSection {
        elements: Vec<BlockElement>,
    },
    RichTextFormatted {
        elements: Vec<BlockElement>,
    },
    Text {
        text: String,
    },
    User {
        user_id: String,
    },
    Button(ButtonBlock),
    Section(SectionBlock),
    Actions(ActionBlock),
    Image(ImageBlock),
    Link(LinkBlock),
    #[serde(other)]
    Unknown,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Block {
    pub block_id: String,
    pub elements: Vec<BlockElement>,
    #[serde(rename = "type")]
    pub ty: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Edited {
    pub ts: StrTimeStamp,
    pub user: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct MessageCommon {
    pub text: String,
    pub ts: StrTimeStamp,
    pub thread_ts: Option<StrTimeStamp>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct BasicMessage {
    #[serde(flatten)]
    pub common: MessageCommon,
    pub channel: String,
    pub user: Option<String>,
    pub bot_id: Option<String>,
    pub edited: Option<Edited>,
    pub event_ts: String,
    pub blocks: Vec<Block>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ChannelJoinMessage {
    #[serde(flatten)]
    pub common: MessageCommon,
    pub user: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct LinksItem {
    pub url: String, //Error("invalid type: string expected a borrowed string", line: 0, column: 0)
    pub domain: String,
}

// No more used, just left for reference
#[derive(Debug, Clone, Deserialize)]
pub struct LinkSharedMessage {
    pub user: String,
    pub channel: String,
    pub message_ts: String,
    pub links: Vec<LinksItem>,
    pub event_ts: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "subtype")]
#[serde(rename_all = "snake_case")]
pub enum TaggedMessage {
    ChannelJoin(ChannelJoinMessage),
    MessageChanged,
    ThreadBroadcast,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum Message {
    BasicMessage(BasicMessage),
    TaggedMessage(TaggedMessage),
    Unknown(serde_json::Value),
}

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type")]
#[serde(rename_all = "snake_case")]
pub enum InternalEvent {
    Message(Message),
    RichText(Message),
    LinkShared(LinkSharedMessage),
    AppMention,
}

#[allow(dead_code)]
#[derive(Debug, Clone, Deserialize)]
pub struct EventCallback {
    pub api_app_id: String,
    pub authed_users: Vec<String>,
    pub event: InternalEvent,
    pub event_id: String,
    pub event_time: NumericTimeStamp,
    pub team_id: String,
    pub token: String,
}

#[allow(dead_code)]
#[derive(Debug, Clone, Deserialize)]
pub struct SlackEventsApi {
    pub envelope_id: String,
    pub payload: Option<EventCallback>,
    pub accepts_response_payload: bool,
    pub retry_attempt: i32,
    pub retry_reason: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum ThreadMessageType {
    Unbroadcasted(ThreadUnbroadcastedMessage),
    Broadcasted(ThreadBroadcastedMessage),
    None(ThreadNoneMessage),
}

#[derive(Debug, Clone, Deserialize)]
pub struct ConversationReplyResponse {
    pub ok: bool,
    pub messages: Option<Vec<ThreadMessageType>>,
    pub error: Option<String>,
    pub has_more: Option<bool>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct ThreadUnbroadcastedMessage {
    #[serde(rename = "type")]
    pub ty: String,
    pub client_msg_id: String,
    pub text: String,
    pub user: String,
    pub ts: StrTimeStamp,
    pub blocks: Vec<BlockElement>,
    pub team: String,
    pub thread_ts: Option<StrTimeStamp>,
    pub parent_user_id: Option<String>,
    pub reply_count: Option<i32>,
    pub reply_users_count: Option<i32>,
    pub latest_reply: Option<StrTimeStamp>,
    pub reply_users: Option<Vec<String>>,
    pub is_locked: Option<bool>,
    pub subscribed: Option<bool>,
    pub last_read: Option<StrTimeStamp>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct ThreadBroadcastedMessage {
    #[serde(rename = "type")]
    pub ty: String,
    pub client_msg_id: Option<String>,
    pub subtype: Option<String>,
    pub text: String,
    pub bot_id: Option<String>,
    pub user: Option<String>,
    pub ts: StrTimeStamp,
    pub thread_ts: StrTimeStamp,
    pub root: ThreadUnbroadcastedMessage,
    pub username: Option<String>,
    pub app_id: Option<String>,
    pub blocks: Vec<BlockElement>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct PostMessageResponse {
    pub ok: bool,
    pub channel: Option<String>,
    pub ts: Option<StrTimeStamp>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct EditMessageResponse {
    pub ok: bool,
    pub channel: Option<String>,
    pub ts: Option<StrTimeStamp>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct ThreadNoneMessage {}

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type")]
#[serde(rename_all = "snake_case")]
pub enum SlackEvent {
    /// https://api.slack.com/events/url_verification
    ///
    /// This event is sent from Slack when the url is first entered.
    /// {
    ///     "token": "TOKEN_VALUE",
    ///     "challenge": "SOME_VALUE",
    ///     "type": "url_verification"
    /// }
    ///
    /// You should reposnd with
    ///
    /// HTTP 200 OK
    /// Content-type: application/x-www-form-urlencoded
    /// challenge=SOME_VALUE
    EventCallback(Box<EventCallback>),
    #[allow(dead_code)]
    UrlVerification {
        token: String,
        challenge: String,
    },
    Hello(SlackHello),
    EventsApi(SlackEventsApi),
    Disconnect {
        reason: String,
    },
}

#[allow(dead_code)]
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct SlackHello {
    pub num_connections: i32,
    pub debug_info: Option<SlackHelloDebugInfo>,
    pub connection_info: Option<SlackHelloConnectionInfo>,
}

#[allow(dead_code)]
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct SlackHelloDebugInfo {
    pub host: String,
    pub build_number: i32,
    pub approximate_connection_time: i32,
}

#[allow(dead_code)]
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct SlackHelloConnectionInfo {
    pub app_id: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct SlackSocketOutput {
    pub payload: Option<serde_json::Value>,
    pub envelope_id: String,
}

/**
 * Sent from client.
 */

#[derive(Debug, Clone, Serialize)]
pub struct PostMessage<'a> {
    pub channel: &'a str,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<&'a str>, // alternative text when blocks are not given (or cannot be displayed).

    #[serde(skip_serializing_if = "Option::is_none")]
    pub blocks: Option<&'a [BlockElement]>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub thread_ts: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub reply_broadcast: Option<bool>,
    // pub as_user: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unfurl_links: Option<bool>,
}

#[derive(Debug, Clone, Serialize)]
pub struct EditMessage<'a> {
    pub channel: &'a str,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<&'a str>, // alternative text when blocks are not given (or cannot be displayed).

    #[serde(skip_serializing_if = "Option::is_none")]
    pub blocks: Option<&'a [BlockElement]>,

    pub ts: String,
}

impl SectionBlock {
    pub fn new_markdown(text: &str) -> Self {
        Self::new_block(text, TextObjectType::Markdown)
    }

    pub fn new_text(text: &str) -> Self {
        Self::new_block(text, TextObjectType::PlainText)
    }

    fn new_block(text: &str, ty: TextObjectType) -> Self {
        Self {
            text: TextObject {
                ty,
                text: text.to_string(),
                emoji: None,
                verbatim: None,
            },
            block_id: None,
            fields: None,
        }
    }
}

#[cfg(test)]
mod test;
