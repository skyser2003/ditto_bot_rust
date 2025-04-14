use anyhow::anyhow;
use anyhow::Context as _;
use async_trait::async_trait;
use axum::body::Body;
use axum::extract::Extension;
use axum::response::IntoResponse;
use axum::response::Response;
use axum::routing::MethodFilter;
use axum::Json;
use bytes::Bytes;
use futures::SinkExt;
use futures::StreamExt;
use log::{debug, error, info, warn};
use reqwest::StatusCode;
use rmcp::model::CallToolRequestParam;
use rmcp::service::RunningService;
use rmcp::transport::TokioChildProcess;
use rmcp::Peer;
use rmcp::RoleClient;
use rmcp::ServiceExt;
use slack::ConversationReplyResponse;
use slack::EditMessage;
use slack::EditMessageResponse;
use slack::PostMessage;
use slack::PostMessageResponse;
use slack::SlackSocketOutput;
use std::borrow::Cow;
use std::collections::HashMap;
use std::sync::Arc;
use std::{
    convert::{TryFrom, TryInto},
    env,
};
use tokio::net::TcpStream;
use tokio::process::Command;
use tokio_tungstenite::tungstenite::Utf8Bytes;
use tokio_tungstenite::MaybeTlsStream;
use tokio_tungstenite::WebSocketStream;
use tokio_tungstenite::{connect_async, tungstenite::protocol::Message as TungsteniteMessage};

mod modules;
mod slack;
#[cfg(test)]
pub mod test;

type McpClient = RunningService<RoleClient, ()>;

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
    #[error("Unsupported")]
    Unsupported(String),
    #[error("Invalid message type")]
    InvalidMessageType(String),
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
            slack::InternalEvent::Message(slack::Message::TaggedMessage(_)) => {
                Err(ConvertMessageEventError::Unsupported(
                    "TaggedMessage event not supported".to_string(),
                ))
            }
            slack::InternalEvent::LinkShared(_) => Err(ConvertMessageEventError::Unsupported(
                "LinkShared event not supported".to_string(),
            )),
            slack::InternalEvent::AppMention => Err(ConvertMessageEventError::Unsupported(
                "AppMention event not supported".to_string(),
            )),
            _ => Err(ConvertMessageEventError::InvalidMessageType(format!(
                "{:?}",
                val
            ))),
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

    async fn get_conversation_replies(
        &self,
        channel: &str,
        ts: &str,
    ) -> anyhow::Result<ConversationReplyResponse>;
    fn redis(&self) -> anyhow::Result<redis::Connection>;

    async fn get_all_tools_metadata(
        &self,
    ) -> anyhow::Result<Vec<(String, HashMap<String, (String, String)>, Vec<String>)>>;

    async fn call_mcp_tool(
        &self,
        name: &str,
        arguments: HashMap<String, serde_json::Value>,
    ) -> anyhow::Result<String>;
}

struct DittoBot {
    bot_id: String,
    bot_token: String,
    openai_key: String,
    gemini_key: String,
    http_client: reqwest::Client,
    redis_client: redis::Client,
    mcp_clients: HashMap<String, McpClient>,
    mcp_tools: HashMap<String, (Cow<'static, str>, Peer<RoleClient>)>,
}

impl DittoBot {
    pub async fn new(
        bot_id: String,
        bot_token: String,
        openai_key: String,
        gemini_key: String,
        redis_client: redis::Client,
        mcp_clients: HashMap<String, McpClient>,
    ) -> Self {
        let mut mcp_tools = HashMap::new();

        for (name, client) in mcp_clients.iter() {
            let tools = client.list_all_tools().await.unwrap_or_default();

            for tool in tools {
                let unified_name = format!("{}_{}", name, tool.name);
                mcp_tools.insert(unified_name, (tool.name, client.peer().clone()));
            }
        }

        Self {
            bot_id,
            bot_token,
            openai_key,
            gemini_key,
            http_client: reqwest::Client::new(),
            redis_client,
            mcp_clients,
            mcp_tools,
        }
    }

    async fn create_mcp_clients(tz: String) -> HashMap<String, McpClient> {
        let mut results = HashMap::new();

        let client1 = async move {
            ().serve(TokioChildProcess::new(
                Command::new("uvx")
                    .arg("mcp-server-time")
                    .arg("--local-timezone")
                    .arg(tz),
            )?)
            .await
        }
        .await;

        results.insert("mcp-server-time", client1);

        results
            .into_iter()
            .filter_map(|(name, client)| match client {
                Ok(client) => Some((name.to_string(), client)),
                Err(e) => {
                    error!("Failed to create mcp client - {:?}", e);
                    None
                }
            })
            .collect()
    }
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

    async fn get_conversation_replies(
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

    async fn get_all_tools_metadata(
        &self,
    ) -> anyhow::Result<Vec<(String, HashMap<String, (String, String)>, Vec<String>)>> {
        let mut datas = vec![];

        for (name, client) in self.mcp_clients.iter() {
            let tools = client
                .list_all_tools()
                .await
                .context("Failed to list all tools")?;

            for tool in tools {
                let unified_name = format!("{}_{}", name, tool.name);

                let properties: &serde_json::Map<String, serde_json::Value> =
                    tool.input_schema["properties"].as_object().unwrap();

                let required = tool.input_schema["required"].as_array().unwrap();
                let required = required
                    .iter()
                    .map(|v| v.as_str().unwrap().to_string())
                    .collect::<Vec<_>>();

                let arguments = properties
                    .keys()
                    .map(|arg_name| {
                        let value = properties.get(arg_name).unwrap();
                        let arg_type = value["type"].as_str().unwrap_or("string");
                        let description = value["description"].as_str().unwrap_or("");

                        (
                            arg_name.to_string(),
                            (arg_type.to_string(), description.to_string()),
                        )
                    })
                    .collect::<HashMap<String, (String, String)>>();

                datas.push((unified_name, arguments, required));
            }
        }

        Ok(datas)
    }

    async fn call_mcp_tool(
        &self,
        unified_name: &str,
        arguments: HashMap<String, serde_json::Value>,
    ) -> anyhow::Result<String> {
        let (tool_name, client) = self
            .mcp_tools
            .get(unified_name)
            .ok_or_else(|| anyhow!("MCP tool not found"))?;

        let mut tool_arguments = serde_json::Map::new();

        for (key, value) in arguments.iter() {
            tool_arguments.insert(key.clone(), value.clone());
        }

        let params = CallToolRequestParam {
            name: tool_name.clone(),
            arguments: Some(tool_arguments),
        };

        let result = client
            .call_tool(params)
            .await
            .context("Failed to call MCP tool")?;

        for content in result.content {
            let text = content.as_text();

            if let Some(text) = text {
                return Ok(text.text.clone());
            }
        }

        error!("No text found in the result content.");

        Ok("".to_string())
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
                    match e {
                        ConvertMessageEventError::Unsupported(_) => {
                            debug!("Unsupported message type - {:?}", e);
                        }
                        ConvertMessageEventError::InvalidMessageType(_) => {
                            error!("Message conversion fail - {:?}", e);
                        }
                    }

                    HttpResponse::Error(StatusCode::BAD_REQUEST)
                }
            }
        }
        _ => {
            error!("Should not be received in http mode - {:?}", event);
            HttpResponse::Error(StatusCode::BAD_REQUEST)
        }
    }
}

async fn socket_handler(mut ws: WebSocketStream<MaybeTlsStream<TcpStream>>, bot: Arc<DittoBot>) {
    while let Some(data) = ws.next().await {
        let data = match data {
            Ok(data) => data,
            Err(e) => {
                error!("Error while receiving data - {:?}", e);
                break;
            }
        };

        match data {
            TungsteniteMessage::Text(text) => {
                debug!("Received text message: {:?}", text);
                let event = serde_json::from_str::<slack::SlackEvent>(&text);

                if event.is_err() {
                    error!("Failed to parse slack event - {:?}", event);
                    continue;
                }

                let event = event.unwrap();

                let mut envelope_id = String::new();

                match &event {
                    slack::SlackEvent::EventsApi(events_api) => {
                        envelope_id = events_api.envelope_id.clone();

                        let payload = &events_api.payload;

                        if payload.is_none() {
                            error!("Payload is None");
                            continue;
                        }

                        let payload = payload.as_ref().unwrap();

                        match (&payload.event).try_into() {
                            Ok(msg) => {
                                let bot = bot.clone();

                                tokio::task::spawn(async move {
                                    if let Err(e) = bot.slack_event_handler(msg).await {
                                        error!(
                                            "Error occured while handling slack event - {:?}",
                                            e
                                        );
                                    }
                                });
                            }
                            Err(e) => match e {
                                ConvertMessageEventError::Unsupported(_) => {
                                    debug!("Unsupported message type - {:?}", e);
                                }
                                ConvertMessageEventError::InvalidMessageType(_) => {
                                    error!("Message conversion fail - {:?}", e);
                                }
                            },
                        }
                    }
                    slack::SlackEvent::Hello(hello) => {
                        debug!("Hello! Number of connections: {}", hello.num_connections);
                    }
                    slack::SlackEvent::Disconnect { reason } => {
                        info!("Disconnect received from slack - {:?}", reason);

                        // Reconnect
                        ws = connect_slack_socket(&bot.bot_token)
                            .await
                            .context("Failed to reconnect to slack socket")
                            .unwrap();

                        info!("Reconnected to slack socket.");
                    }
                    _ => {
                        error!("Should not be received in socket mode - {:?}", event);
                    }
                }

                // If event is not hello
                // send ack to slack
                match &event {
                    slack::SlackEvent::Hello(_) => {}
                    _ => {
                        let ack = SlackSocketOutput {
                            envelope_id,
                            payload: None,
                        };

                        ws.send(TungsteniteMessage::Text(Utf8Bytes::from(
                            serde_json::to_string(&ack).unwrap(),
                        )))
                        .await
                        .unwrap();
                    }
                }
            }
            TungsteniteMessage::Ping(_) => {
                debug!("Received ping message");
                ws.send(TungsteniteMessage::Pong(Bytes::from("Pong from ditto")))
                    .await
                    .unwrap();
            }
            etc => {
                debug!("Received non-text message: {:?}", etc);
            }
        }
    }
}

async fn connect_slack_socket(
    app_token: &str,
) -> anyhow::Result<WebSocketStream<MaybeTlsStream<TcpStream>>> {
    let socket_url = "https://slack.com/api/apps.connections.open";
    let client = reqwest::Client::new();

    let response = client
        .post(socket_url)
        .header("Authorization", format!("Bearer {}", app_token))
        .send()
        .await?
        .json::<serde_json::Value>()
        .await?;

    if response["ok"].as_bool() != Some(true) {
        return Err(anyhow!(
            "Failed to open socket connection: {}",
            response["error"]
        ));
    }

    let url = response["url"]
        .as_str()
        .ok_or_else(|| anyhow!("Invalid socket URL"))?;

    info!("Socket connection url: {:?}", url);

    let (ws, _) = connect_async(url)
        .await
        .context("Failed to connect to slack websocket.")?;

    info!("Connected to slack websocket.");

    Ok(ws)
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    env_logger::init();

    let bot_token = env::var("SLACK_BOT_TOKEN").context("Bot token is not given")?;
    info!("Bot token: {:?}", bot_token);

    let app_token = env::var("SLACK_APP_TOKEN").context("App token is not given")?;
    info!("App token: {:?}", app_token);

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

    let socket_mode = env::var("SOCKET_MODE").unwrap_or("0".to_string());
    info!("Socket mode: {:?}", socket_mode);

    let tz = env::var("TZ").unwrap_or("Asia/Seoul".to_string());

    let is_socket_mode = socket_mode == "1" || socket_mode.to_lowercase() == "true";
    info!("Is socket mode: {:?}", is_socket_mode);

    let app = axum::Router::new().route(
        "/",
        axum::routing::on(MethodFilter::POST | MethodFilter::GET, http_handler),
    );

    let mcp_clients = DittoBot::create_mcp_clients(tz).await;

    let bot = Arc::new(
        DittoBot::new(
            bot_id.clone(),
            bot_token.clone(),
            openai_key.clone(),
            gemini_key.clone(),
            redis::Client::open(format!("redis://{}", redis_address))
                .context("Failed to create redis client")?,
            mcp_clients,
        )
        .await,
    );

    if is_socket_mode {
        info!("Start using slack socket mode.");

        let ws = connect_slack_socket(&app_token)
            .await
            .context("Failed to connect to slack socket")?;

        socket_handler(ws, bot).await;
    } else {
        let app = app.layer(Extension(bot));
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
    }

    Ok(())
}

// curl 'https://slack.com/api/chat.postMessage' -H 'Authorization: Bearer SECRET' -H 'Content-type: application/json; charset=utf-8' -d '{"channel": "CS2AVF83X", "text": "hello, world"}'
