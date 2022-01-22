use anyhow::Context as _;
use async_trait::async_trait;
use axum::body::Body;
use axum::extract::Extension;
use axum::response::IntoResponse;
use axum::response::Response;
use axum::routing::MethodFilter;
use axum::{AddExtensionLayer, Json};
use log::{debug, error, info, warn};
use reqwest::StatusCode;
use slack::PostMessage;
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
    user: String,
    channel: String,
    text: String,
    link: Option<String>,
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
            slack::InternalEvent::Message(slack::Message::BasicMessage(msg)) => Ok(Self {
                user: msg.user.to_string(),
                channel: msg.channel.to_string(),
                text: msg.common.text.to_string(),
                link: None,
            }),
            slack::InternalEvent::LinkShared(msg) => Ok(Self {
                user: msg.user.to_string(),
                channel: msg.channel.to_string(),
                text: "".to_string(),
                link: Some(msg.links[0].url.to_string()),
            }),
            _ => Err(ConvertMessageEventError::InvalidMessageType),
        }
    }
}

pub enum Message<'a> {
    Blocks(&'a [slack::BlockElement]),
    Text(&'a str),
}

impl<'a> Message<'a> {
    fn as_postmessage(&self, channel: &'a str) -> PostMessage<'a> {
        match self {
            Message::Blocks(blocks) => slack::PostMessage {
                channel,
                text: None,
                blocks: Some(blocks),
            },
            Message::Text(text) => slack::PostMessage {
                channel,
                text: Some(text),
                blocks: None,
            },
        }
    }
}

#[async_trait]
pub trait Bot {
    fn bot_id(&self) -> &'_ str;
    fn bot_token(&self) -> &'_ str;
    async fn send_message(&self, channel: &str, msg: Message<'_>) -> anyhow::Result<()>;
    fn redis(&self) -> redis::Connection;
}

struct DittoBot {
    bot_id: String,
    bot_token: String,
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

    async fn send_message(&self, channel: &str, message: Message<'_>) -> anyhow::Result<()> {
        let builder = self
            .http_client
            .post("https://slack.com/api/chat.postMessage")
            .header("Content-type", "application/json; charset=utf-8")
            .header("Authorization", format!("Bearer {}", &self.bot_token));

        let reply = message.as_postmessage(channel);

        let resp = builder
            .json(&reply)
            .send()
            .await
            .context("Failed to send request")?;
        debug!(
            "Response from reply: {:?}",
            resp.text().await.context("Failed to read body")?
        );
        Ok(())
    }

    fn redis(&self) -> redis::Connection {
        self.redis_client
            .get_connection()
            .unwrap_or_else(|_| unsafe { std::hint::unreachable_unchecked() })
    }
}

impl DittoBot {
    async fn slack_event_handler(&self, msg: MessageEvent) -> anyhow::Result<()> {
        if msg.user.contains(&self.bot_id) {
            debug!("Bot id is in user: {:?}, {:?}", msg.user, self.bot_id);
            return Ok(());
        }

        macro_rules! invoke_modules {
            (($self:ident, $msg:ident) => [$($(#[cfg($meta:meta)])? $module:path),*]) => {
                tokio::join!($(invoke_modules!(@mod $($meta,)? $self, $msg, $module)),*)
            };
            (@mod $meta:meta, $self:ident, $msg:ident, $module:path) => {{
                #[cfg($meta)]
                let m = $module($self, &$msg);
                #[cfg(not($meta))]
                let m = futures::future::ok::<(), anyhow::Error>(());
                invoke_modules!(@log_error $module => m)
            }};
            (@mod $self:ident, $msg:ident, $module:path) => {
                invoke_modules!(@log_error $module => $module($self, &$msg))
            };
            (@log_error $module:path => $($body:tt)+) => {
                futures::TryFutureExt::unwrap_or_else($($body)+, |e| {
                    error!("Module {} returned error - {}", stringify!($module), e);
                })
            };
        }

        invoke_modules!(
            (self, msg) => [
                modules::surplus::handle,
                modules::mhw::handle,
                modules::namuwiki::handle,
                modules::ph::handle
            ]
        );

        Ok(())
    }
}

#[derive(Debug, thiserror::Error)]
enum Error {
    #[error("{0}")]
    UnspecifiedError(#[from] anyhow::Error),
    #[error("event parse failed")]
    EventParsingError,
}

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
    Json(event): Json<slack::SlackEvent>,
    Extension(bot): Extension<Arc<DittoBot>>,
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

    let app = axum::Router::new().route(
        "/",
        axum::routing::on(MethodFilter::POST | MethodFilter::GET, http_handler),
    );
    let app = app.layer(AddExtensionLayer::new(Arc::new(DittoBot {
        bot_id,
        bot_token,
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
