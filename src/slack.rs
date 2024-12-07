pub mod protocol;
#[cfg(test)]
mod test;

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
    BlockElement, EditMessage, InternalEvent, Message as SlackMessage, PostMessage, SlackEvent,
};
use reqwest::StatusCode;

use crate::{ConvertMessageEventError, DittoBot, Message, MessageEvent, ReplyMessageEvent};

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
                    channel: msg.channel.to_string(),
                    text: msg.common.text.to_string(),
                    ts,
                    thread_ts: if let Some(thread_ts) = msg.common.thread_ts.clone() {
                        Some(String::from(&thread_ts))
                    } else {
                        None
                    },
                    link,
                })
            }
            _ => Err(ConvertMessageEventError::InvalidMessageType),
        }
    }
}

impl<'a> super::Message<'a> {
    pub fn as_postmessage(
        &self,
        channel: &'a str,
        reply: Option<ReplyMessageEvent>,
        unfurl_links: Option<bool>,
    ) -> PostMessage<'a> {
        let (thread_ts, reply_broadcast) = match reply {
            Some(reply) => (Some(reply.msg), Some(reply.broadcast)),
            None => (None, None),
        };

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

    pub fn as_editmessage(&self, channel: &'a str, ts: &'a str) -> EditMessage<'a> {
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
