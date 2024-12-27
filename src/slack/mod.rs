mod protocol;
use crate::bot::DittoBot;
use crate::message::{ConvertMessageEventError, Message, MessageEvent, ReplyMessageEvent};
use anyhow::Context;
use axum::{
    body::Body,
    extract::Extension,
    response::{IntoResponse, Response},
    routing::MethodFilter,
    Json,
};
use log::{debug, error, info, warn};
pub use protocol::*;
use reqwest::StatusCode;
use std::convert::{TryFrom, TryInto};
use std::sync::Arc;

impl TryFrom<&protocol::InternalEvent> for MessageEvent {
    type Error = ConvertMessageEventError;

    fn try_from(val: &protocol::InternalEvent) -> std::result::Result<Self, Self::Error> {
        match val {
            protocol::InternalEvent::Message(protocol::Message::BasicMessage(msg)) => {
                let mut link_url: Option<&String> = None;

                msg.blocks.iter().any(|block| {
                    block.elements.iter().any(|element| match element {
                        protocol::BlockElement::Link(link_block) => {
                            link_url = Some(&link_block.url);
                            true
                        }
                        protocol::BlockElement::RichTextSection { elements } => {
                            elements.iter().any(|element| match element {
                                protocol::BlockElement::Link(link_block) => {
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

impl<'a> Message<'a> {
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

#[cfg(feature = "check-req")]
mod auth;

enum HttpResponse {
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

async fn http_handler<'a>(
    Extension(bot): Extension<Arc<DittoBot>>,
    Json(event): Json<protocol::SlackEvent>,
) -> HttpResponse {
    debug!("Parsed Event: {:?}", event);

    match event {
        protocol::SlackEvent::UrlVerification { challenge, .. } => {
            HttpResponse::Challenge(challenge)
        }
        protocol::SlackEvent::EventCallback(event_callback) => {
            match (&event_callback.event).try_into() {
                Ok(msg) => {
                    tokio::task::spawn(async move {
                        if let Err(e) = bot.slack_event_handler(msg).await {
                            error!("Error occured while handling slack event - {:?}", e);
                        }
                    });
                    HttpResponse::Ok
                }
                Err(e) => {
                    error!("Message conversion fail - {:?}", e);

                    HttpResponse::Error(StatusCode::BAD_REQUEST)
                }
            }
        }
    }
}

pub async fn run() -> anyhow::Result<()> {
    use std::env;

    let bot_token = env::var("SLACK_BOT_TOKEN").context("Bot token is not given")?;
    info!("Bot token: {:?}", bot_token);

    let bot_id = env::var("BOT_ID").context("Bot id is not given")?;
    let redis_address = env::var("REDIS_ADDRESS").context("Redis address is not given")?;

    info!("Slack bot id: {:?}", bot_id);
    info!("Redis address: {:?}", redis_address);

    let openai_key = env::var("OPENAI_KEY").context("OpenAI key is not given")?;
    info!("OpenAI Key: {:?}", openai_key);

    let openai_model = env::var("OPENAI_MODEL").unwrap_or("gpt-4".to_string());
    info!("OpenAI model: {:?}", openai_model);

    let gemini_key = env::var("GEMINI_KEY").context("Gemini key is not given")?;
    info!("Gemini Key: {:?}", gemini_key);

    let app = axum::Router::new().route(
        "/",
        axum::routing::on(MethodFilter::POST | MethodFilter::GET, http_handler),
    );
    let app = app.layer(Extension(Arc::new(DittoBot {
        bot_id,
        bot_token,
        openai_key,
        gemini_key,
        http_client: reqwest::Client::new(),
        redis_client: redis::Client::open(format!("redis://{}", redis_address))
            .context("Failed to create redis client")?,
    })));
    #[cfg(feature = "check-req")]
    let app = app.layer(tower_http::auth::AsyncRequireAuthorizationLayer::new({
        let signing_secret = env::var("SLACK_SIGNING_SECRET")
            .context("Signing secret is not given.")?
            .into_bytes();
        auth::SlackAuthorization::new(signing_secret)
    }));

    let use_ssl = env::var("USE_SSL")
        .ok()
        .and_then(|v| {
            if cfg!(feature = "use-ssl") {
                v.parse().ok()
            } else {
                warn!("use-ssl feature is disabled!. USE_SSL env will be ignored");
                Some(false)
            }
        })
        .unwrap_or(false);
    if use_ssl {
        #[cfg(feature = "use-ssl")]
        {
            use axum_server::tls_rustls::RustlsConfig;
            use axum_server::Handle;

            info!("Start to bind address with ssl.");
            let config = RustlsConfig::from_pem_file("PUBLIC_KEY.pem", "PRIVATE_KEY.pem")
                .await
                .context("Fail to open pem files")?;

            let handle = Handle::new();
            let handle_for_ctrl = handle.clone();

            tokio::spawn(async move {
                tokio::signal::ctrl_c()
                    .await
                    .expect("Failed to listen signal.");
                info!("Gracefully shutdown...");
                handle_for_ctrl.graceful_shutdown(None);
            });

            axum_server::bind_rustls("0.0.0.0:14475".parse()?, config)
                .handle(handle)
                .serve(app.into_make_service())
                .await?;
        }
    } else {
        info!("Start to bind address with HTTP.");
        axum::Server::bind(&"0.0.0.0:8082".parse()?)
            .serve(app.into_make_service())
            .with_graceful_shutdown(futures::FutureExt::map(tokio::signal::ctrl_c(), |_| ()))
            .await?;
    }

    Ok(())
}

#[cfg(test)]
mod test;
