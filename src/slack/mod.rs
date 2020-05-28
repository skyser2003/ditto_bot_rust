use serde_derive::{Deserialize, Serialize};

#[derive(Debug, Deserialize, Serialize)]
pub struct TextObject<'a> {
    #[serde(rename = "type")]
    pub ty: &'a str, //plain_text or mkdwn
    pub text: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub emoji: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub verbatim: Option<bool>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct SectionBlock<'a> {
    pub text: TextObject<'a>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub block_id: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fields: Option<Vec<TextObject<'a>>>,
    //pub accessory:
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ButtonBlock<'a> {
    #[serde(rename = "type")]
    pub ty: &'a str,
    pub action_id: &'a str,
    pub url: Option<&'a str>,
    pub value: Option<&'a str>,
    pub style: Option<&'a str>, //primary or danger
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(tag = "type")]
#[serde(rename_all = "snake_case")]
pub enum BlockElement<'a> {
    RichText {
        block_id: &'a str,
        elements: Vec<Box<BlockElement<'a>>>,
    },
    RichTextSection {
        elements: Vec<Box<BlockElement<'a>>>,
    },
    Text {
        text: String,
    },
    Button(ButtonBlock<'a>),
    Section(SectionBlock<'a>),
}

#[derive(Debug, Deserialize)]
pub struct Block<'a> {
    pub block_id: &'a str,
    pub elements: Vec<BlockElement<'a>>,
    #[serde(rename = "type")]
    pub ty: &'a str,
}

#[derive(Debug, Deserialize)]
pub struct Reaction {
    pub count: u32,
    pub name: String,
    pub users: Vec<String>,
}

#[derive(Debug, Deserialize)]
pub struct Edited<'a> {
    pub ts: &'a str,
    pub user: &'a str,
}

#[derive(Debug, Deserialize)]
pub struct Icons<'a> {
    pub image_36: Option<&'a str>,
    pub image_48: Option<&'a str>,
    pub image_72: Option<&'a str>,
}

#[derive(Debug, Deserialize)]
pub struct MessageCommon<'a> {
    pub text: String,
    pub ts: &'a str,
}

#[derive(Debug, Deserialize)]
pub struct LinksEvent {
    pub url: String,
    pub domain: String
}

#[derive(Debug, Deserialize)]
pub struct BasicMessage<'a> {
    #[serde(flatten)]
    pub common: MessageCommon<'a>,
    pub channel: &'a str,
    pub user: &'a str,
    pub edited: Option<Edited<'a>>,
}

#[derive(Debug, Deserialize)]
pub struct BotMessage<'a> {
    #[serde(flatten)]
    pub common: MessageCommon<'a>,
    pub bot_id: &'a str
}

#[derive(Debug, Deserialize)]
pub struct ChannelJoinMessage<'a> {
    #[serde(flatten)]
    pub common: MessageCommon<'a>,
    pub user: &'a str,
}


/*
#[derive(Debug, Deserialize)]
pub struct LinkSharedMessage<'a> {
    #[serde(flatten)]
    pub common: MessageCommon<'a>,
    pub links: [LinksEvent; 1]
}
*/

#[derive(Debug)]
// tag: subtype
pub enum Message<'a> {
    BotMessage(BotMessage<'a>),
    ChannelJoin(ChannelJoinMessage<'a>),
    BasicMessage(BasicMessage<'a>)
}

impl<'de> serde::Deserialize<'de> for Message<'de> {
    fn deserialize<D>(deserializer: D) -> Result<Message<'de>, D::Error>
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
                            println!("Subtype: {}", v.as_str().unwrap());

                            if tag.is_some() {
                                Err(serde::de::Error::duplicate_field("subtype"))?;
                            }
                            tag = Some(match v.as_str() {
                                Some(subtype) => subtype.to_string(),
                                None => Err(serde::de::Error::missing_field("subtype"))?,
                            });
                        }
                        (k, v) => {
                            println!("Key: {}", k);
                            vec.push((Content::Str(k), v));
                        }
                    }
                }
                Ok(TaggedContent {
                    tag: tag.unwrap_or("".to_string()),
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
                    _ => Err(serde::de::Error::unknown_variant(&tagged.tag, &[$($field),*]))?
                }
            }
        }
        deserialize_subtype! {
            "bot_message" => BotMessage<'de> as Message::BotMessage,
            "channel_join" => ChannelJoinMessage<'de> as Message::ChannelJoin,
            "" => BasicMessage<'de> as Message::BasicMessage,
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
#[serde(rename_all = "snake_case")]
pub enum InternalEvent<'a> {
    Message(#[serde(borrow)] Message<'a>),
}

#[derive(Debug, Deserialize)]
pub struct EventCallback<'a> {
    pub api_app_id: &'a str,
    pub authed_users: Vec<&'a str>,
    pub event: InternalEvent<'a>,
    pub event_id: &'a str,
    pub event_time: u64,
    pub team_id: &'a str,
    pub token: &'a str,
}

#[derive(Debug, Deserialize)]
pub struct LinkSharedCallback<'a> {
    pub api_app_id: &'a str,
    pub event: InternalEvent<'a>
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
#[serde(rename_all = "snake_case")]
pub enum SlackEvent<'a> {
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
    EventCallback(EventCallback<'a>),
    UrlVerification {
        token: &'a str,
        challenge: &'a str,
    },
    LinkEvent(LinkSharedCallback<'a>)
}

/**
 * Sent from client.
 */

#[derive(Debug, Serialize)]
pub struct PostMessage<'a> {
    pub channel: &'a str,
    pub text: &'a str, // alternative text when blocks are not given (or cannot be displayed).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub blocks: Option<Vec<BlockElement<'a>>>,
    // pub as_user: Option<bool>,
}
