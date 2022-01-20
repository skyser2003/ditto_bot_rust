use anyhow::Context as _;
use async_trait::async_trait;
use axum::body::Body;
use axum::extract::Extension;
use axum::response::IntoResponse;
use axum::response::Response;
use axum::routing::MethodFilter;
use axum::{AddExtensionLayer, Json};
use log::{debug, error, info};
use reqwest::StatusCode;
use std::sync::Arc;
use std::{
    convert::{TryFrom, TryInto},
    env,
};

mod modules;
mod slack;

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

#[async_trait]
pub trait Bot {
    fn bot_id(&self) -> &'_ str;
    fn bot_token(&self) -> &'_ str;
    async fn send_message(
        &self,
        channel: &str,
        blocks: &[slack::BlockElement],
    ) -> anyhow::Result<()>;

    #[cfg(feature = "surplus")]
    fn redis(&self) -> redis::Connection;
}

struct DittoBot {
    bot_id: String,
    bot_token: String,
    http_client: reqwest::Client,

    #[cfg(feature = "surplus")]
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

    async fn send_message(
        &self,
        channel: &str,
        blocks: &[slack::BlockElement],
    ) -> anyhow::Result<()> {
        let builder = self
            .http_client
            .post("https://slack.com/api/chat.postMessage")
            .header("Content-type", "application/json; charset=utf-8")
            .header("Authorization", format!("Bearer {}", &self.bot_token));

        let reply = slack::PostMessage {
            channel,
            text: None,
            blocks: Some(blocks),
        };

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


    #[cfg(feature = "surplus")]
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
            ($self:ident, $msg:ident => $($module:path),*) => {
                tokio::join!($($module($self, &$msg),)*)
            };
        }

        invoke_modules!(self, msg => modules::surplus::handle, modules::mhw::handle, modules::namuwiki::handle);

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

#[cfg(feature = "check-req")]
mod auth {
    use axum::{
        body::{boxed, Body, BoxBody},
        http::{Request, Response, StatusCode},
    };
    use futures::future::BoxFuture;
    use hmac::Mac;
    use log::debug;

    struct ByteBuf<'a>(&'a [u8]);

    impl<'a> std::fmt::LowerHex for ByteBuf<'a> {
        fn fmt(&self, fmtr: &mut std::fmt::Formatter) -> std::result::Result<(), std::fmt::Error> {
            for byte in self.0 {
                fmtr.write_fmt(format_args!("{:02x}", byte))?;
            }
            Ok(())
        }
    }

    #[derive(Debug, Clone)]

    pub struct SlackAuthorization(pub Vec<u8>);

    async fn authorize<B: axum::body::HttpBody + Unpin + Send + Sync + 'static>(
        secret: &[u8],
        mut request: Request<B>,
    ) -> Result<Request<B>, Response<BoxBody>> {
        let data = request.body().data();

        fn empty_response(status_code: StatusCode) -> axum::http::Response<BoxBody> {
            axum::http::Response::builder()
                .status(status_code)
                .body(boxed(Body::empty()))
                .unwrap_or_else(|_| unsafe { std::hint::unreachable_unchecked() })
        }
        let headers = request.headers();
        let (signature, timestamp) =
            if let (Some(s), Some(t)) = (headers.get("X-Slack-Signature"), headers.get("X-")) {
                (s, t)
            } else {
                return Err(empty_response(StatusCode::BAD_REQUEST));
            };
        let signature = signature.as_bytes();
        let timestamp = timestamp
            .to_str()
            .map_err(|_| empty_response(StatusCode::BAD_REQUEST))?;

        {
            let cur_timestamp = std::time::SystemTime::now()
                .duration_since(std::time::SystemTime::UNIX_EPOCH)
                .unwrap()
                .checked_sub(std::time::Duration::from_secs(
                    timestamp.parse::<u64>().unwrap(),
                ));

            debug!("now: {:?}", cur_timestamp.unwrap());
            //TODO: check replay attack
        }

        let mut mac = hmac::Hmac::<sha2::Sha256>::new_from_slice(secret)
            .unwrap_or_else(|_| unsafe { std::hint::unreachable_unchecked() });
        mac.update("v0:".as_bytes());
        mac.update(timestamp.as_bytes());
        mac.update(data.await?);

        let calculated_signature = format!("v0={:02x}", ByteBuf(&mac.finalize().into_bytes()));

        if signature != calculated_signature.as_bytes() {
            return Err(empty_response(StatusCode::UNAUTHORIZED));
        }

        debug!("Success to verify a slack's signature.");

        Ok(request)
    }

    impl<B: axum::body::HttpBody + Unpin + Send + Sync + 'static>
        tower_http::auth::AsyncAuthorizeRequest<B> for SlackAuthorization
    {
        type RequestBody = B;
        type ResponseBody = BoxBody;
        type Future = BoxFuture<'static, Result<Request<B>, Response<Self::ResponseBody>>>;

        fn authorize(&mut self, mut request: Request<B>) -> Self::Future {
            Box::pin(authorize(&self.0, request))
        }
    }
}

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
        slack::SlackEvent::UrlVerification { challenge, .. } => {
            HttpResponse::Challenge(challenge.to_string())
        }
        slack::SlackEvent::EventCallback(event_callback) => {
            match (&event_callback.event).try_into() {
                Ok(msg) => {
                    let bot = bot.clone();
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
    let redis_address = if cfg!(feature = "redis") {
        env::var("REDIS_ADDRESS").context("Redis address is not given")?
    } else {
        "NONE".to_string()
    };

    info!("Slack bot id: {:?}", bot_id);
    info!("Redis address: {:?}", redis_address);

    let app = axum::Router::new()
        .route(
            "/",
            axum::routing::on(MethodFilter::POST | MethodFilter::GET, http_handler),
        )
        .layer(AddExtensionLayer::new(Arc::new(DittoBot {
            bot_id,
            bot_token,
            http_client: reqwest::Client::new(),
            #[cfg(feature = "surplus")]
            redis_client: redis::Client::open(format!("redis://{}", redis_address))
                .context("Failed to create redis client")?,
        })));
    #[cfg(feature = "check-req")]
    let app = app.layer(tower_http::auth::RequireAuthorizationLayer::custom({
        let signing_secret = env::var("SLACK_SIGNING_SECRET")
            .context("Signing secret is not given.")?
            .into_bytes();
        auth::SlackAuthorization(signing_secret)
    }));

    // let use_ssl = env::var("USE_SSL")
    //     .ok()
    //     .and_then(|v| v.parse().ok())
    //     .unwrap_or(false);
    // if use_ssl {
    //     // info!("Trying to bind ssl.");
    //     // let mut config = ServerConfig::new(NoClientAuth::new());
    //     // let mut cert_file = BufReader::new(
    //     //     File::open("PUBLIC_KEY.pem")
    //     //         .context("SSL enabled. but failed to open PUBLIC_KEY.pem")?,
    //     // );
    //     // let mut key_file = BufReader::new(
    //     //     File::open("PRIVATE_KEY.pem")
    //     //         .context("SSL enabled. but failed to open PRIVATE_KEY.pem")?,
    //     // );
    //     // let cert_chain = certs(&mut cert_file).map_err(|_| anyhow!("Failed to parse certs"))?;
    //     // let mut keys =
    //     //     rsa_private_keys(&mut key_file).map_err(|_| anyhow!("Failed to parse private keys"))?;

    //     // if keys.is_empty() {
    //     //     panic!("Fails to load a private key file. Check it is formatted in RSA.")
    //     // }
    //     // config.set_single_cert(cert_chain, keys.remove(0))?;

    //     // http_srv = http_srv.bind_rustls("0.0.0.0:14475", config)?;
    // }
    axum::Server::bind(&"0.0.0.0:8082".parse()?)
        .serve(app.into_make_service())
        .with_graceful_shutdown(futures::FutureExt::map(tokio::signal::ctrl_c(), |_| ()))
        .await?;

    Ok(())
}

// curl 'https://slack.com/api/chat.postMessage' -H 'Authorization: Bearer SECRET' -H 'Content-type: application/json; charset=utf-8' -d '{"channel": "CS2AVF83X", "text": "hello, world"}'
