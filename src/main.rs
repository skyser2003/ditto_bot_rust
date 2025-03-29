use anyhow::anyhow;
use anyhow::Context as _;
use async_trait::async_trait;
use axum::body::Body;
use axum::extract::Extension;
use axum::response::IntoResponse;
use axum::response::Response;
use axum::routing::MethodFilter;
use axum::Json;
use log::{debug, error, info, warn};
use reqwest::StatusCode;
use slack::ConversationReplyResponse;
use slack::EditMessage;
use slack::EditMessageResponse;
use slack::PostMessage;
use slack::PostMessageResponse;
use std::sync::Arc;
use std::{
    convert::{TryFrom, TryInto},
    env,
};

mod modules;
mod slack;
#[cfg(test)]
pub mod test;

pub struct MessageEvent {
    is_bot: bool,
    user: String,
    channel: String,
    text: String,
    ts: String,
    thread_ts: Option<String>,
    link: Option<String>,
}

#[derive(Clone)]
pub struct ReplyMessageEvent {
    msg: String,
    broadcast: bool,
}

#[derive(Debug, thiserror::Error)]
pub enum ConvertMessageEventError {
    #[error("Invalid message type")]
    InvalidMessageType,
}

impl TryFrom<&slack::InternalEvent> for MessageEvent {
    type Error = ConvertMessageEventError;

    fn try_from(val: &slack::InternalEvent) -> std::result::Result<Self, Self::Error> {
        match val {
            slack::InternalEvent::Message(slack::Message::BasicMessage(msg)) => {
                let mut link_url: Option<&String> = None;

                msg.blocks.iter().any(|block| {
                    block.elements.iter().any(|element| match element {
                        slack::BlockElement::Link(link_block) => {
                            link_url = Some(&link_block.url);
                            true
                        }
                        slack::BlockElement::RichTextSection { elements } => {
                            elements.iter().any(|element| match element {
                                slack::BlockElement::Link(link_block) => {
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

pub enum Message<'a> {
    Blocks(&'a [slack::BlockElement]),
    Text(&'a str),
}

impl<'a> Message<'a> {
    fn as_postmessage(
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
            Message::Blocks(blocks) => slack::PostMessage {
                channel,
                text: None,
                blocks: Some(blocks),
                thread_ts,
                reply_broadcast,
                unfurl_links,
            },
            Message::Text(text) => slack::PostMessage {
                channel,
                text: Some(text),
                blocks: None,
                thread_ts,
                reply_broadcast,
                unfurl_links,
            },
        }
    }

    fn as_editmessage(&self, channel: &'a str, ts: &'a str) -> EditMessage<'a> {
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

#[async_trait]
pub trait Bot {
    fn bot_id(&self) -> &'_ str;
    fn bot_token(&self) -> &'_ str;
    fn openai_key(&self) -> &'_ str;
    fn gemini_key(&self) -> &'_ str;

    async fn send_message(
        &self,
        channel: &str,
        msg: Message<'_>,
        reply: Option<ReplyMessageEvent>,
        unfurl_links: Option<bool>,
    ) -> anyhow::Result<PostMessageResponse>;

    async fn edit_message(
        &self,
        channel: &str,
        msg: Message<'_>,
        ts: &str,
    ) -> anyhow::Result<EditMessageResponse>;

    async fn get_conversation_relies(
        &self,
        channel: &str,
        ts: &str,
    ) -> anyhow::Result<ConversationReplyResponse>;
    fn redis(&self) -> anyhow::Result<redis::Connection>;
}

struct DittoBot {
    bot_id: String,
    bot_token: String,
    openai_key: String,
    gemini_key: String,
    http_client: reqwest::Client,
    redis_client: redis::Client,
}

#[async_trait]
impl Bot for DittoBot {
    fn bot_id(&self) -> &'_ str {
        &self.bot_id
    }

    fn bot_token(&self) -> &'_ str {
        &self.bot_token
    }

    fn openai_key(&self) -> &'_ str {
        &self.openai_key
    }

    fn gemini_key(&self) -> &'_ str {
        &self.gemini_key
    }

    async fn send_message(
        &self,
        channel: &str,
        message: Message<'_>,
        reply: Option<ReplyMessageEvent>,
        unfurl_links: Option<bool>,
    ) -> anyhow::Result<PostMessageResponse> {
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
            .json::<PostMessageResponse>()
            .await
            .context("Failed to parse response")?;

        Ok(resp)
    }

    async fn edit_message(
        &self,
        channel: &str,
        message: Message<'_>,
        ts: &str,
    ) -> anyhow::Result<EditMessageResponse> {
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
            .json::<EditMessageResponse>()
            .await
            .context("Failed to parse response")?;

        Ok(resp)
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

    fn redis(&self) -> anyhow::Result<redis::Connection> {
        self.redis_client
            .get_connection()
            .context("Failed to get redis connection")
    }
}

impl DittoBot {
    async fn slack_event_handler(&self, msg: MessageEvent) -> anyhow::Result<()> {
        if msg.is_bot || msg.user.contains(&self.bot_id) {
            debug!("Ignoring bot message");
            return Ok(());
        }

        modules::invoke_all_modules(self, msg).await;

        Ok(())
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
    Json(event): Json<slack::SlackEvent>,
) -> HttpResponse {
    debug!("Parsed Event: {:?}", event);

    match event {
        slack::SlackEvent::UrlVerification { challenge, .. } => HttpResponse::Challenge(challenge),
        slack::SlackEvent::EventCallback(event_callback) => {
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

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    env_logger::init();

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

// curl 'https://slack.com/api/chat.postMessage' -H 'Authorization: Bearer SECRET' -H 'Content-type: application/json; charset=utf-8' -d '{"channel": "CS2AVF83X", "text": "hello, world"}'
