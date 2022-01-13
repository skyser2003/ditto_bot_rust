use std::{
    fmt::{Debug, Formatter},
    time::{Duration, SystemTime},
};

use serde::{Deserialize, Serialize};

#[derive(Deserialize, Serialize, PartialEq, Eq)]
pub struct StrTimeStamp(String);

impl Into<SystemTime> for &StrTimeStamp {
    fn into(self) -> SystemTime {
        SystemTime::UNIX_EPOCH + Duration::from_secs_f32(self.0.parse().unwrap())
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

impl Into<SystemTime> for &NumericTimeStamp {
    fn into(self) -> SystemTime {
        SystemTime::UNIX_EPOCH + Duration::from_secs(self.0)
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

#[derive(Debug)]
// tag: subtype
pub enum Message {
    BotMessage(BotMessage),
    ChannelJoin(ChannelJoinMessage),
    BasicMessage(BasicMessage),
}

impl<'de> serde::Deserialize<'de> for Message {
    fn deserialize<D>(deserializer: D) -> Result<Message, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        use serde::private::de::Content;
        use std::fmt;
        struct FieldVisitor;
        struct TaggedContent<'de> {
            pub tag: String,
            pub content: Content<'de>,
        }
        impl<'de> ::serde::de::Visitor<'de> for FieldVisitor {
            type Value = TaggedContent<'de>;
            fn expecting(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
                fmt.write_str("subtyped or non-subtyped message")
            }

            fn visit_map<M>(self, mut map: M) -> Result<Self::Value, M::Error>
            where
                M: serde::de::MapAccess<'de>,
            {
                let mut tag = None;
                let mut vec = Vec::with_capacity(30);
                while let Some(kv) = map.next_entry::<&'de str, Content<'de>>()? {
                    match kv {
                        ("subtype", v) => {
                            if tag.is_some() {
                                return Err(serde::de::Error::duplicate_field("subtype"));
                            }
                            tag = Some(match v.as_str() {
                                Some(subtype) => subtype.to_string(),
                                None => return Err(serde::de::Error::missing_field("subtype")),
                            });
                        }
                        (k, v) => {
                            vec.push((Content::Str(k), v));
                        }
                    }
                }
                Ok(TaggedContent {
                    tag: tag.unwrap_or_else(|| "".to_string()),
                    content: Content::Map(vec),
                })
            }
        }
        impl<'de> ::serde::Deserialize<'de> for TaggedContent<'de> {
            #[inline]
            fn deserialize<__D>(d: __D) -> ::serde::export::Result<Self, __D::Error>
            where
                __D: ::serde::Deserializer<'de>,
            {
                ::serde::Deserializer::deserialize_identifier(d, FieldVisitor)
            }
        }
        let tagged = match ::serde::Deserializer::deserialize_any(deserializer, FieldVisitor) {
            ::serde::export::Ok(val) => val,
            ::serde::export::Err(e) => {
                return ::serde::export::Err(e);
            }
        };

        macro_rules! deserialize_subtype {
            ($($field:expr => $e_type:ty as $v_type:path,)*) => {
                match tagged.tag.as_str() {
                    $($field => ::serde::export::Result::map(
                        <$e_type as ::serde::Deserialize>::deserialize(
                            ::serde::private::de::ContentDeserializer::<D::Error>::new(
                                tagged.content,
                            ),
                        ),
                        $v_type,
                    ),)*
                    _ => Err(serde::de::Error::unknown_variant(&tagged.tag, &[$($field),*]))
                }
            }
        }
        deserialize_subtype! {
            "bot_message" => BotMessage as Message::BotMessage,
            "channel_join" => ChannelJoinMessage as Message::ChannelJoin,
            "" => BasicMessage as Message::BasicMessage,
        }
    }
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
    EventCallback(EventCallback),
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
