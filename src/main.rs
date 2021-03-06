use actix::prelude::*;
use actix::{fut::wrap_future, System};
use actix_web::http::StatusCode;
use actix_web::{web, App, HttpRequest, HttpResponse, HttpServer, Result};
use anyhow::Error;
use futures::executor;
use hmac::{Hmac, Mac};
use lazy_static::lazy_static;
use log::{debug, error, info};
use rustls::internal::pemfile::{certs, rsa_private_keys};
use rustls::{NoClientAuth, ServerConfig};
use sha2::Sha256;
use std::env;
use std::fs::File;
use std::{
    convert::{TryFrom, TryInto},
    io::BufReader,
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

impl TryFrom<&slack::InternalEvent<'_>> for MessageEvent {
    type Error = ConvertMessageEventError;

    fn try_from(val: &slack::InternalEvent) -> Result<Self, Self::Error> {
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

impl Message for MessageEvent {
    type Result = Result<(), Error>;
}

struct SlackEventActor {
    bot_token: String,
    bot_id: String,
    slack_client: reqwest::Client,
    redis_client: redis::Client,
}

trait SlackMessageSender
where
    Self: Actor,
{
    fn base_request_builder(&self) -> reqwest::RequestBuilder;
    fn send(&mut self, context: &mut Self::Context, channel: &str, blocks: &[slack::BlockElement]);

    fn generate_request(
        request_builder: reqwest::RequestBuilder,
        channel: &str,
        blocks: &[slack::BlockElement<'_>],
    ) -> reqwest::RequestBuilder;
}

impl Actor for SlackEventActor {
    type Context = Context<Self>;
}

async fn send_request(request_builder: reqwest::RequestBuilder) {
    let resp = request_builder.send().await;
    debug!(
        "Response from reply: {:?}",
        resp.unwrap().text().await.unwrap()
    );
}

impl SlackMessageSender for SlackEventActor {
    fn base_request_builder(&self) -> reqwest::RequestBuilder {
        self.slack_client
            .post("https://slack.com/api/chat.postMessage")
            .header("Content-type", "application/json; charset=utf-8")
            .header("Authorization", format!("Bearer {}", &self.bot_token))
    }

    fn send(
        &mut self,
        context: &mut Self::Context,
        channel: &str,
        blocks: &[slack::BlockElement<'_>],
    ) {
        let base_request_builder = self.base_request_builder();
        let request_builder = Self::generate_request(base_request_builder, channel, &blocks);

        context.spawn(wrap_future(send_request(request_builder)));
    }

    fn generate_request(
        request_builder: reqwest::RequestBuilder,
        channel: &str,
        blocks: &[slack::BlockElement<'_>],
    ) -> reqwest::RequestBuilder {
        let reply = slack::PostMessage {
            channel,
            text: None,
            blocks: Some(blocks),
        };

        request_builder.json(&reply)
    }
}

lazy_static! {
    pub static ref IS_TEST: bool = env::var("TEST")
        .map(|test_val| test_val == "1")
        .unwrap_or(false);
    static ref REDIS_ADDRESS: String = env::var("REDIS_ADDRESS").unwrap_or_else(|_| "".to_string());
    static ref BOT_ID: String = env::var("BOT_ID").unwrap_or_else(|_| "".to_string());
}

impl Handler<MessageEvent> for SlackEventActor {
    type Result = Result<(), Error>;

    fn handle(&mut self, msg: MessageEvent, context: &mut Self::Context) -> Self::Result {
        if msg.user.contains(&self.bot_id) {
            return Ok(());
        }

        let text = msg.text;
        let user = msg.user;
        let link = msg.link;
        let channel = msg.channel;

        let mut blocks = Vec::<slack::BlockElement>::new();

        let mut conn = self.redis_client.get_connection().unwrap();

        let base_request_builder = self.base_request_builder();

        context.spawn(wrap_future(async move {
            modules::surplus::handle(&text, &user, &mut blocks, &mut conn, &*BOT_ID)
                .await
                .unwrap_or_else(|err| {
                    error!("Chat redis record fail: {}", err);
                });

            modules::mhw::handle(&text, &mut blocks);
            modules::namuwiki::handle(link, &mut blocks).await;

            let request = Self::generate_request(base_request_builder, &channel, &blocks);
            send_request(request).await;
        }));
        Ok(())
    }
}

struct AppState {
    sender: Addr<SlackEventActor>,
    signing_secret: String,
}

struct ByteBuf<'a>(&'a [u8]);

impl<'a> std::fmt::LowerHex for ByteBuf<'a> {
    fn fmt(&self, fmtr: &mut std::fmt::Formatter) -> Result<(), std::fmt::Error> {
        for byte in self.0 {
            fmtr.write_fmt(format_args!("{:02x}", byte))?;
        }
        Ok(())
    }
}

async fn normal_handler(
    req: HttpRequest,
    body: String,
    state: web::Data<AppState>,
) -> Result<HttpResponse> {
    debug!("Body: {:?}", body);

    let content_str = if let Some(i) = req.headers().get("content-type") {
        i.to_str().unwrap()
    } else {
        ""
    };

    if !(*IS_TEST) {
        let (slack_signature, slack_timestamp) = if let (Some(sig), Some(ts)) = (
            req.headers()
                .get("X-Slack-Signature")
                .and_then(|s| s.to_str().ok()),
            req.headers()
                .get("X-Slack-Request-Timestamp")
                .and_then(|s| s.to_str().ok()),
        ) {
            (sig, ts)
        } else {
            return Ok(HttpResponse::Unauthorized().finish());
        };

        {
            let cur_timestamp = std::time::SystemTime::now()
                .duration_since(std::time::SystemTime::UNIX_EPOCH)
                .unwrap()
                .checked_sub(std::time::Duration::from_secs(
                    slack_timestamp.parse::<u64>().unwrap(),
                ));

            debug!("now: {:?}", cur_timestamp.unwrap());
            //TODO: check replay attack
        }

        let signature_base_string: String = format!("v0:{}:{}", slack_timestamp, body);

        let mut mac = Hmac::<Sha256>::new_varkey(state.signing_secret.as_bytes()).expect("");
        mac.input(signature_base_string.as_bytes());

        let calculated_signature = format!("v0={:02x}", ByteBuf(&mac.result().code().as_slice()));

        if slack_signature != calculated_signature {
            return Ok(HttpResponse::Unauthorized().finish());
        }
        debug!("Success to verify a slack's signature.");
    }

    if !content_str.contains("json") {
        return Ok(HttpResponse::build(StatusCode::BAD_REQUEST)
            .content_type("text/html; charset=utf-8")
            .body(body));
    }

    let posted_event: slack::SlackEvent = match serde_json::from_str(&body) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("Failed to parse a slack json object: {:?}", e);
            return Ok(HttpResponse::build(StatusCode::OK)
                .content_type("text/html; charset=utf-8")
                .body(body));
        }
    };

    debug!("Parsed Event: {:?}", posted_event);

    match posted_event {
        slack::SlackEvent::UrlVerification { challenge, .. } => {
            Ok(HttpResponse::build(StatusCode::OK)
                .content_type("application/x-www-form-urlencoded")
                .body(challenge.to_string()))
        }
        slack::SlackEvent::EventCallback(event_callback) => {
            match (&event_callback.event).try_into() {
                Ok(msg) => {
                    state.sender.do_send(msg);

                    Ok(HttpResponse::build(StatusCode::OK)
                        .content_type("text/html; charset=utf-8")
                        .body(body))
                }
                Err(e) => {
                    error!("Message conversion fail - {:?}", e);
                    Ok(HttpResponse::BadRequest().finish())
                }
            }
        }
    }
}

fn main() -> std::io::Result<()> {
    env_logger::init();

    let bot_token = match env::var("SLACK_BOT_TOKEN") {
        Ok(val) => val,
        Err(_e) => panic!("Bot token is not given."),
    };
    info!("Bot token: {:?}", bot_token);

    let signing_secret = match env::var("SLACK_SIGNING_SECRET") {
        Ok(val) => val,
        Err(_e) => panic!("Signing secret is not given."),
    };

    let system = System::new("slack");

    let slack_event_actor = SlackEventActor {
        bot_token,
        bot_id: "URS3HL8SD".to_string(), //TODO: remove hardcoded value
        slack_client: reqwest::Client::new(),
        redis_client: redis::Client::open(format!("redis://{}", *REDIS_ADDRESS)).unwrap(),
    }
    .start();

    let (tx, rx) = std::sync::mpsc::channel();

    let _ = std::thread::spawn(move || {
        let system = System::new("http");

        let mut http_srv = HttpServer::new(move || {
            App::new()
                .data(AppState {
                    sender: slack_event_actor.clone(),
                    signing_secret: signing_secret.clone(),
                })
                .route("/", web::post().to(normal_handler))
                .route("/", web::get().to(normal_handler))
        });

        let use_ssl = match env::var("USE_SSL") {
            Ok(val) => val.parse().unwrap_or(false),
            Err(_e) => false,
        };

        if use_ssl {
            info!("Trying to bind ssl.");
            let mut config = ServerConfig::new(NoClientAuth::new());
            let cert_file = &mut BufReader::new(File::open("PUBLIC_KEY.pem").unwrap());
            let key_file = &mut BufReader::new(File::open("PRIVATE_KEY.pem").unwrap());
            let cert_chain = certs(cert_file).unwrap();
            let mut keys = rsa_private_keys(key_file).unwrap();

            if keys.is_empty() {
                panic!("Fails to load a private key file. Check it is formatted in RSA.")
            }
            config.set_single_cert(cert_chain, keys.remove(0)).unwrap();

            http_srv = http_srv.bind_rustls("0.0.0.0:14475", config).unwrap();
        } else {
            info!("Trying to bind http.");

            http_srv = http_srv.bind("0.0.0.0:8082").unwrap();
        }

        let srv = http_srv.run();

        let _ = tx.send(srv);

        info!("Server run start!");

        system.run()
    });

    let srv = rx.recv().unwrap();
    let main_sys = System::current();
    let ctrl_c_pressed = std::sync::atomic::AtomicBool::new(false);

    ctrlc::set_handler(move || {
        if ctrl_c_pressed.load(std::sync::atomic::Ordering::SeqCst) {
            info!("Force to stop program.");
            std::process::exit(1);
        }
        ctrl_c_pressed.store(true, std::sync::atomic::Ordering::SeqCst);
        info!("Try to stop HttpServer.");
        executor::block_on(srv.stop(true));
        main_sys.stop();
        info!("Stopped.");
    })
    .expect("Fail to set Ctrl-C handler.");

    system.run()
}

// curl 'https://slack.com/api/chat.postMessage' -H 'Authorization: Bearer SECRET' -H 'Content-type: application/json; charset=utf-8' -d '{"channel": "CS2AVF83X", "text": "hello, world"}'
