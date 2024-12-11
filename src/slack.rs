pub mod protocol;
#[cfg(test)]
mod test;

use anyhow::{anyhow, Context as _};
use std::{
    convert::{TryFrom, TryInto},
    sync::Arc,
};

use axum::{
    body::Body,
    response::{IntoResponse, Response},
    Extension, Json,
};
use log::{debug, error};
use protocol::{
    BlockElement, ConversationReplyResponse, EditMessage, InternalEvent, Message as SlackMessage,
    PostMessage, SlackEvent,
};
use reqwest::StatusCode;

use crate::{
    message::{MessageId, MessageSource},
    ConvertMessageEventError, DittoBot, Message, MessageEvent,
};

pub struct Bot {
    bot_id: String,
    bot_token: String,
    http_client: reqwest::Client,
}

impl Bot {
    pub fn new(bot_id: String, bot_token: String) -> Self {
        Self {
            bot_id,
            bot_token,
            http_client: reqwest::Client::new(),
        }
    }
}

#[derive(Clone)]
pub struct ReplyMessageEvent {
    msg: String,
    broadcast: bool,
}

impl Bot {
    pub async fn send_message(
        &self,
        channel: &str,
        thread_ts: Option<&str>,
        broadcast: Option<bool>,
        message: &crate::message::Message,
    ) -> anyhow::Result<MessageId> {
        let builder = self
            .http_client
            .post("https://slack.com/api/chat.postMessage")
            .header("Content-type", "application/json; charset=utf-8")
            .header("Authorization", format!("Bearer {}", &self.bot_token));

        let reply = message.as_postmessage(channel, reply, unfurl_links);

        let resp = builder
            .json(&reply)
            .send()
            .await
            .context("Failed to send request")?;

        let resp = resp
            .json::<protocol::PostMessageResponse>()
            .await
            .context("Failed to parse response")?;

        if !resp.ok {
            Err(anyhow!(
                "Slack chat.postMessage Failed - {}",
                resp.error
                    .unwrap_or_else(|| "[EMPTY ERROR MESSAGE]".to_string())
            ))
        } else {
            Ok(MessageId::SlackMessage(From::from(&resp.ts.unwrap())))
        }
    }

    async fn edit_message(
        &self,
        channel: &str,
        ts: &str,
        message: &crate::message::Message,
    ) -> anyhow::Result<MessageId> {
        let builder = self
            .http_client
            .post("https://slack.com/api/chat.update")
            .header("Content-type", "application/json; charset=utf-8")
            .header("Authorization", format!("Bearer {}", &self.bot_token));

        let body = message.as_editmessage(channel, ts);

        let resp = builder
            .json(&body)
            .send()
            .await
            .context("Failed to send request")?;

        let resp = resp
            .json::<protocol::EditMessageResponse>()
            .await
            .context("Failed to parse response")?;

        if !resp.ok {
            Err(anyhow!(
                "Slack chat.update Failed - {}",
                resp.error
                    .unwrap_or_else(|| "[EMPTY ERROR MESSAGE]".to_string())
            ))
        } else {
            Ok(MessageId::SlackMessage(From::from(&resp.ts.unwrap())))
        }
    }

    async fn get_conversation_relies(
        &self,
        channel: &str,
        ts: &str,
    ) -> anyhow::Result<ConversationReplyResponse> {
        let builder = self
            .http_client
            .get("https://slack.com/api/conversations.replies")
            .header("Content-type", "application/json; charset=utf-8")
            .header("Authorization", format!("Bearer {}", &self.bot_token))
            .query(&[("channel", channel), ("ts", ts)]);

        let res = builder.send().await.context("Failed to send request")?;

        let body = res.text().await?;

        let json_result = serde_json::from_str::<ConversationReplyResponse>(&body);

        if json_result.is_ok() {
            Ok(json_result.unwrap())
        } else {
            Err(anyhow!(
                "Json parsing failed for conversations.replies: {:?} {}",
                json_result.err(),
                body
            ))
        }
    }
}

impl TryFrom<&InternalEvent> for MessageEvent {
    type Error = ConvertMessageEventError;

    fn try_from(val: &InternalEvent) -> std::result::Result<Self, Self::Error> {
        match val {
            InternalEvent::Message(SlackMessage::BasicMessage(msg)) => {
                let mut link_url: Option<&String> = None;

                msg.blocks.iter().any(|block| {
                    block.elements.iter().any(|element| match element {
                        BlockElement::Link(link_block) => {
                            link_url = Some(&link_block.url);
                            true
                        }
                        BlockElement::RichTextSection { elements } => {
                            elements.iter().any(|element| match element {
                                BlockElement::Link(link_block) => {
                                    link_url = Some(&link_block.url);

                                    true
                                }
                                _ => false,
                            })
                        }
                        _ => false,
                    })
                });

                let (ts, link) = if let Some(link) = link_url {
                    (msg.event_ts.clone(), Some(link.clone()))
                } else {
                    (String::from(&msg.common.ts), None)
                };

                Ok(Self {
                    is_bot: msg.bot_id.is_some(),
                    user: msg
                        .user
                        .clone()
                        .unwrap_or(msg.bot_id.clone().unwrap_or_default()),
                    source: if let Some(thread_ts) = msg.common.thread_ts {
                        MessageSource::SlackThread {
                            channel: msg.channel.clone(),
                            thread_ts: String::from(&thread_ts),
                        }
                    } else {
                        MessageSource::SlackChannel(msg.channel.to_string())
                    },
                    id: MessageId::SlackMessage(ts),
                    text: msg.common.text.to_string(),
                    link,
                })
            }
            _ => Err(ConvertMessageEventError::InvalidMessageType),
        }
    }
}

impl crate::message::Message {
    pub fn as_postmessage<'a>(
        &self,
        channel: &'a str,
        reply_ts: Option<&'a str>,
        broadcast: bool,
        unfurl_links: Option<bool>,
    ) -> PostMessage<'a> {
        let thread_ts = reply_ts.map(str::to_string);
        let reply_broadcast = reply_ts.is_some().then_some(broadcast);

        match self {
            Message::Blocks(blocks) => PostMessage {
                channel,
                text: None,
                blocks: Some(blocks),
                thread_ts,
                reply_broadcast,
                unfurl_links,
            },
            Message::Text(text) => PostMessage {
                channel,
                text: Some(text),
                blocks: None,
                thread_ts,
                reply_broadcast,
                unfurl_links,
            },
        }
    }

    pub fn as_editmessage<'a>(&self, channel: &'a str, ts: &'a str) -> EditMessage<'a> {
        match self {
            Message::Blocks(blocks) => EditMessage {
                channel,
                text: None,
                blocks: Some(blocks),
                ts: ts.to_string(),
            },
            Message::Text(text) => EditMessage {
                channel,
                text: Some(text),
                blocks: None,
                ts: ts.to_string(),
            },
        }
    }
}

pub enum HttpResponse {
    Challenge(String),
    Ok,
    Error(StatusCode),
}

impl IntoResponse for HttpResponse {
    fn into_response(self) -> Response {
        match self {
            HttpResponse::Challenge(s) => Response::builder()
                .status(StatusCode::OK)
                .body(axum::body::boxed(Body::from(format!("challenge={}", s)))),
            HttpResponse::Ok => Response::builder()
                .status(StatusCode::OK)
                .body(axum::body::boxed(Body::empty())),
            HttpResponse::Error(status_code) => Response::builder()
                .status(status_code)
                .body(axum::body::boxed(Body::empty())),
        }
        .unwrap_or_else(|_| unsafe { std::hint::unreachable_unchecked() })
    }
}

pub async fn http_handler<'a>(
    Extension(bot): Extension<Arc<DittoBot>>,
    Json(event): Json<SlackEvent>,
) -> HttpResponse {
    debug!("Parsed Event: {:?}", event);

    match event {
        SlackEvent::UrlVerification { challenge, .. } => HttpResponse::Challenge(challenge),
        SlackEvent::EventCallback(event_callback) => match (&event_callback.event).try_into() {
            Ok(msg) => {
                tokio::task::spawn(async move {
                    if let Err(e) = bot.handle_message_event(msg).await {
                        error!("Error occured while handling slack event - {:?}", e);
                    }
                });
                HttpResponse::Ok
            }
            Err(e) => {
                error!("Message conversion fail - {:?}", e);

                HttpResponse::Error(StatusCode::BAD_REQUEST)
            }
        },
    }
}
