use anyhow::{Context as _};
use async_trait::async_trait;
use axum::{extract::Extension, routing::MethodFilter};
use log::{debug, error, info, warn};
use message::{Message, MessageEvent, MessageId, MessageTarget};
use std::{env, sync::Arc};

mod discord;
mod message;
mod modules;
mod slack;

#[cfg(test)]
pub mod test;

#[derive(Debug, thiserror::Error)]
pub enum ConvertMessageEventError {
    #[error("Invalid message type")]
    InvalidMessageType,
}

pub struct Config {
    openai_key: String,
    gemini_key: String,
}

trait ExtractShared: Send + Sync + Clone {
    fn extract_from_shared(shared: &Shared) -> Self;
}

macro_rules! declare_shared {
    (pub struct $name:ident { $($item_name:ident: $item_ty:ty,)+ }) => {
        pub struct $name {
            $($item_name: $item_ty,)+
        }

        $(
            impl ExtractShared for $item_ty {
                fn extract_from_shared(shared: &Shared) -> $item_ty {
                    shared.$item_name.clone()
                }
            }
        )+
    };
}

declare_shared! {
    pub struct Shared {
        config: Arc<Config>,
        redis_client: redis::Client,
    }
}

trait BotHandler<T> {
    type Future: std::future::Future<Output = anyhow::Result<()>> + Send + 'static;

    fn call(self, bot: &DittoBot, message: &Message) -> Self::Future;
}

macro_rules! impl_handlers {
    ($(($($name:ident),*);)+) => {
        $(
            #[allow(non_snake_case, unused_mut)]
            impl<F, Fut, $($name),*> BotHandler<($($name,)*)> for F
                where
                    F: FnOnce(&DittoBot, &Message, $($name),*) -> Fut + Clone + Send + 'static,
                    Fut: std::future::Future<Output = anyhow::Result<()>> + Send {
                type Future = std::pin::Pin<Box<dyn std::future::Future<Output = anyhow::Result<()>> + Send>>;

                fn call(self, bot: &DittoBot, message: &Message) -> Self::Future {
                    Box::pin(async move {
                        $(
                            let $name = $name::extract_from_shared(&bot.shared);
                        )*

                        self(bot, message, $($name),*).await
                    })
                }
            }
        )+
    };
}

impl_handlers! {
    ();
    (T);
    (T1, T2);
    (T1, T2, T3);
    (T1, T2, T3, T4);
    (T1, T2, T3, T4, T5);
}

struct DittoBot {
    shared: Arc<Shared>,
    slack: slack::Bot,
}

#[async_trait]
trait Bot {
    async fn send_message(
        &self,
        target: MessageTarget,
        message: &Message,
    ) -> anyhow::Result<MessageId>;
    async fn get_conversation(&self, message_id: MessageId) -> anyhow::Result<Vec<MessageEvent>>;
}

#[async_trait]
impl Bot for DittoBot {
    async fn send_message(
        &self,
        target: MessageTarget,
        message: &Message,
    ) -> anyhow::Result<MessageId> {
        match target {
            MessageTarget::SlackChannel(channel) => todo!(),
            MessageTarget::SlackThread {
                channel,
                thread_ts,
                broadcast,
            } => todo!(),
            MessageTarget::SlackEditMessage { channel, ts } => todo!(),
            MessageTarget::DiscordChannel(channel_id) => todo!(),
        }
    }

    async fn get_conversation(&self, message_id: MessageId) -> anyhow::Result<Vec<MessageEvent>> {
        todo!()
    }
}

impl DittoBot {
    async fn handle_message_event(&self, msg: MessageEvent) -> anyhow::Result<()> {
        if msg.is_bot {
            debug!("Ignoring bot message");
            return Ok(());
        }

        modules::invoke_all_modules(self, msg).await;

        Ok(())
    }
}

#[cfg(feature = "check-req")]
mod auth;

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
        axum::routing::on(MethodFilter::POST | MethodFilter::GET, slack::http_handler),
    );
    let app = app.layer(Extension(Arc::new(DittoBot {
        shared: Arc::new(Shared {
            config: Arc::new(Config {
                openai_key,
                gemini_key,
            }),
            redis_client: redis::Client::open(format!("redis://{}", redis_address))
                .context("Failed to create redis client")?,
        }),
        slack: slack::Bot::new(bot_id, bot_token),
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
