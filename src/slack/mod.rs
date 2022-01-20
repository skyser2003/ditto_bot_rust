use std::{
    fmt::{Debug, Formatter},
    time::{Duration, SystemTime},
};

use serde::{Deserialize, Serialize};

#[derive(Deserialize, Serialize, PartialEq, Eq)]
pub struct StrTimeStamp(String);

impl From<&StrTimeStamp> for SystemTime {
    fn from(val: &StrTimeStamp) -> SystemTime {
        SystemTime::UNIX_EPOCH + Duration::from_secs_f32(val.0.parse().unwrap())
    }
}

impl Debug for StrTimeStamp {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        let t: SystemTime = self.into();
        write!(f, "{}({:?})", self.0, t)
    }
}

#[derive(Deserialize, Serialize, PartialEq, Eq)]
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

#[derive(Debug, Deserialize, Serialize, PartialEq, Eq)]
pub enum TextObjectType {
    #[serde(rename = "plain_text")]
    PlainText,
    #[serde(rename = "mrkdwn")]
    Markdown,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct TextObject {
    #[serde(rename = "type")]
    pub ty: TextObjectType,
    pub text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub emoji: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub verbatim: Option<bool>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct SectionBlock {
    pub text: TextObject,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub block_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fields: Option<Vec<TextObject>>, //pub accessory:
}

#[derive(Debug, Deserialize, Serialize)]
pub struct ActionBlock {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub block_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub elements: Option<Vec<BlockElement>>,
}

#[derive(Debug, Deserialize, Serialize)]
pub enum ButtonStyle {
    #[serde(rename = "primary")]
    Primary,
    #[serde(rename = "danger")]
    Danger,
}

#[derive(Debug, Serialize, Deserialize)]
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

#[derive(Debug, Serialize, Deserialize)]
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

#[derive(Debug, Deserialize, Serialize)]
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
    Text {
        text: String,
    },
    Button(ButtonBlock),
    Section(SectionBlock),
    Actions(ActionBlock),
    Image(ImageBlock),
}

#[derive(Debug, Deserialize)]
pub struct Block {
    pub block_id: String,
    pub elements: Vec<BlockElement>,
    #[serde(rename = "type")]
    pub ty: String,
}

#[derive(Debug, Deserialize)]
pub struct Reaction {
    pub count: u32,
    pub name: String,
    pub users: Vec<String>,
}

#[derive(Debug, Deserialize)]
pub struct Edited {
    pub ts: StrTimeStamp,
    pub user: String,
}

#[derive(Debug, Deserialize)]
pub struct Icons {
    pub image_36: Option<String>,
    pub image_48: Option<String>,
    pub image_72: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct MessageCommon {
    pub text: String,
    pub ts: StrTimeStamp,
}

#[derive(Debug, Deserialize)]
pub struct BasicMessage {
    #[serde(flatten)]
    pub common: MessageCommon,
    pub channel: String,
    pub user: String,
    pub edited: Option<Edited>,
}

#[derive(Debug, Deserialize)]
pub struct BotMessage {
    #[serde(flatten)]
    pub common: MessageCommon,
    pub bot_id: String,
}

#[derive(Debug, Deserialize)]
pub struct ChannelJoinMessage {
    #[serde(flatten)]
    pub common: MessageCommon,
    pub user: String,
}

#[derive(Debug, Deserialize)]
pub struct LinksItem {
    pub url: String, //Error("invalid type: string expected a borrowed string", line: 0, column: 0)
    pub domain: String,
}

#[derive(Debug, Deserialize)]
pub struct LinkSharedMessage {
    pub user: String,
    pub channel: String,
    pub message_ts: String,
    pub links: Vec<LinksItem>,
    pub event_ts: String,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "subtype")]
#[serde(rename_all = "snake_case")]
pub enum TaggedMessage {
    BotMessage(BotMessage),
    ChannelJoin(ChannelJoinMessage),
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub enum Message {
    TaggedMessage(TaggedMessage),
    BasicMessage(BasicMessage),
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
#[serde(rename_all = "snake_case")]
pub enum InternalEvent {
    Message(Message),
    LinkShared(LinkSharedMessage),
}

#[derive(Debug, Deserialize)]
pub struct EventCallback {
    pub api_app_id: String,
    pub authed_users: Vec<String>,
    pub event: InternalEvent,
    pub event_id: String,
    pub event_time: NumericTimeStamp,
    pub team_id: String,
    pub token: String,
}

#[derive(Debug, Deserialize)]
pub struct UsersList {
    pub members: Vec<Member>,
}

#[derive(Debug, Deserialize)]
pub struct Member {
    pub id: String,
    pub name: String,
}

#[derive(Debug, Deserialize)]
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
    UrlVerification {
        token: String,
        challenge: String,
    },
}

/**
 * Sent from client.
 */

#[derive(Debug, Serialize)]
pub struct PostMessage<'a> {
    pub channel: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<&'a str>, // alternative text when blocks are not given (or cannot be displayed).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub blocks: Option<&'a [BlockElement]>,
    // pub as_user: Option<bool>,
}

#[cfg(test)]
mod test;
