use actix::prelude::*;
use actix::{fut::wrap_future, System};
use actix_web::http::StatusCode;
use actix_web::{web, App, HttpRequest, HttpResponse, HttpServer, Result};
use anyhow::{anyhow, Error};
use ctrlc;
use futures::executor;
use hmac::{Hmac, Mac};
use lazy_static::lazy_static;
use regex::Regex;
use reqwest;
use rustls::internal::pemfile::{certs, rsa_private_keys};
use rustls::{NoClientAuth, ServerConfig};
use sha2::Sha256;
use std::borrow::Cow;
use std::env;
use std::fs::File;
use std::io::BufReader;
use url::Url;

mod slack;

struct MessageEvent {
    user: String,
    channel: String,
    text: String,
    link: Option<String>,
}

impl MessageEvent {
    fn from_slack_event(msg: &slack::Message) -> Result<Self, Error> {
        match msg {
            slack::Message::BasicMessage(msg) => Ok(Self {
                user: msg.user.to_string(),
                channel: msg.channel.to_string(),
                text: msg.common.text.to_string(),
                link: None,
            }),
            _ => Err(anyhow!("Invalid event")),
        }
    }
    fn from_slack_link_event(msg: &slack::LinkSharedMessage) -> Result<Self, Error> {
        Ok(Self {
            user: msg.user.to_string(),
            channel: msg.channel.to_string(),
            text: msg.common.text.to_string(),
            link: Option::Some(msg.links[0].url.to_string()),
        })
    }
}

impl Message for MessageEvent {
    type Result = Result<(), Error>;
}

struct SlackEventActor {
    bot_token: String,
    bot_id: String,
    slack_client: reqwest::Client,
}

trait SlackMessageSender
where
    Self: Actor,
{
    fn base_request_builder(&self) -> reqwest::RequestBuilder;
    fn send(
        &mut self,
        context: &mut Self::Context,
        channel: &str,
        blocks: &Vec<slack::BlockElement>,
    );

    fn generate_request(
        request_builder: reqwest::RequestBuilder,
        channel: &str,
        blocks: &Vec<slack::BlockElement<'_>>,
    ) -> reqwest::RequestBuilder;
}

impl Actor for SlackEventActor {
    type Context = Context<Self>;
}

async fn send_request(request_builder: reqwest::RequestBuilder) {
    let resp = request_builder.send().await;
    println!(
        "Response from reply: {:?}",
        resp.unwrap().text().await.unwrap()
    );
}

impl SlackMessageSender for SlackEventActor {
    fn base_request_builder(&self) -> reqwest::RequestBuilder {
        self.slack_client
            .post("https://slack.com/api/chat.postMessage")
            .header("Content-type", "application/json; charset=utf-8")
            .header("Authorization", "Bearer ".to_string() + &self.bot_token)
    }

    fn send(
        &mut self,
        context: &mut Self::Context,
        channel: &str,
        blocks: &Vec<slack::BlockElement<'_>>,
    ) {
        let base_request_builder = self.base_request_builder();
        let request_builder = Self::generate_request(base_request_builder, channel, &blocks);

        context.spawn(wrap_future(send_request(request_builder)));
    }

    fn generate_request(
        request_builder: reqwest::RequestBuilder,
        channel: &str,
        blocks: &Vec<slack::BlockElement<'_>>,
    ) -> reqwest::RequestBuilder {
        let reply = slack::PostMessage {
            channel: channel,
            text: None,
            blocks: Some(blocks),
        };

        request_builder.json(&reply)
    }
}

lazy_static! {
    static ref TITLE_REGEX: Regex = Regex::new(r"<title>(.+) - 나무위키</title>").unwrap();
    static ref MHW_DATA: Vec<slack::MonsterHunterData<'static>> = vec![
        slack::MonsterHunterData {
            keywords: vec!["ㄷㄷ", "ㄷㄷ가마루", "도도가마루"],
            text: "도도가마루",
            image_url: "https://github.com/shipduck/ditto_bot/blob/master/images/Dodogama.png"
        },
        slack::MonsterHunterData {
            keywords: vec!["ㅊㅊ", "추천"],
            text: "치치야크",
            image_url: "https://github.com/shipduck/ditto_bot/blob/master/images/Tzitzi_Ya_Ku.png"
        },
        slack::MonsterHunterData {
            keywords: vec!["ㅈㄹ", "지랄"],
            text: "조라마그다라오스",
            image_url:
                "https://github.com/shipduck/ditto_bot/blob/master/images/Zorah_Magdaros.png"
        },
        slack::MonsterHunterData {
            keywords: vec!["ㄹㅇ", "리얼"],
            text: "로아루드로스",
            image_url: "https://github.com/shipduck/ditto_bot/blob/master/images/Royal_Ludroth.png"
        },
        slack::MonsterHunterData {
            keywords: vec!["ㅇㄷ"],
            text: "오도가론",
            image_url: "https://github.com/shipduck/ditto_bot/blob/master/images/Odogaron.png"
        },
        slack::MonsterHunterData {
            keywords: vec!["이불", "졸려", "잘래", "잠와", "이블조"],
            text: "이블조",
            image_url: "https://github.com/shipduck/ditto_bot/blob/master/images/Evil_Jaw.png"
        },
    ];
}

impl Handler<MessageEvent> for SlackEventActor {
    type Result = Result<(), Error>;

    fn handle(&mut self, msg: MessageEvent, context: &mut Self::Context) -> Self::Result {
        if msg.user.contains(&self.bot_id) {
            return Ok(());
        }

        let base_request_builder = self.base_request_builder();

        context.spawn(wrap_future(async move {
            let mut blocks = Vec::<slack::BlockElement>::new();

            // Mhw images
            for data in MHW_DATA.iter() {
                for keyword in data.keywords.iter() {
                    if msg.text.contains(keyword) {
                        blocks.push(slack::BlockElement::Image(slack::ImageBlock {
                            ty: "image",
                            image_url: data.image_url,
                            alt_text: data.text,
                            title: None,
                            block_id: None,
                        }));
                    }
                }
            }

            // Namuwiki
            match &msg.link {
                Some(link) => {
                    let parsed_url = Url::parse(link).unwrap();
                    let url_string = parsed_url.host_str().unwrap();

                    if url_string != "namu.wiki" {
                        return;
                    }

                    let res = reqwest::get(link).await.unwrap();
                    let body = res.text().await.unwrap();

                    let title_opt = TITLE_REGEX.captures(&body).and_then(|captures| {
                        captures
                            .get(1)
                            .and_then(|match_title| Some(match_title.as_str()))
                    });

                    let title = match title_opt {
                        Some(val) => format!("{} - 나무위키", val),
                        None => "Invalid url".to_string(),
                    };

                    blocks.push(slack::BlockElement::Actions(slack::ActionBlock {
                        block_id: None,
                        elements: Some(vec![slack::BlockElement::Button(slack::ButtonBlock {
                            text: slack::TextObject {
                                ty: "plain_text",
                                text: Cow::from(title),
                                emoji: None,
                                verbatim: None,
                            },
                            action_id: None,
                            url: Some(link),
                            value: None,
                            style: Some("primary"),
                        })]),
                    }));
                }
                _ => {}
            }

            let request = Self::generate_request(base_request_builder, &msg.channel, &blocks);
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
    println!("Body: {:?}", body);

    let content_str = if let Some(i) = req.headers().get("content-type") {
        i.to_str().unwrap()
    } else {
        ""
    };

    let is_test = match env::var("TEST") {
        Ok(test_val) => match test_val.as_ref() {
            "1" => true,
            _ => false,
        },
        Err(_) => false,
    };

    if is_test == false {
        let slack_signature: &str = if let Some(sig) = req.headers().get("X-Slack-Signature") {
            sig.to_str().unwrap()
        } else {
            return Ok(HttpResponse::Unauthorized().finish());
        };

        let slack_timestamp: &str =
            if let Some(sig) = req.headers().get("X-Slack-Request-Timestamp") {
                sig.to_str().unwrap()
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

            println!("now: {:?}", cur_timestamp.unwrap());
            //TODO: check replay attack
        }

        let signature_base_string: String = format!("v0:{}:{}", slack_timestamp, body);

        let mut mac = Hmac::<Sha256>::new_varkey(state.signing_secret.as_bytes()).expect("");
        mac.input(signature_base_string.as_bytes());

        let calculated_signature = format!("v0={:02x}", ByteBuf(&mac.result().code().as_slice()));

        if slack_signature != calculated_signature {
            return Ok(HttpResponse::Unauthorized().finish());
        }
        println!("Success to verify a slack's signature.");
    }

    if content_str.contains("json") {
        let posted_event: slack::SlackEvent = match serde_json::from_str(&body) {
            Ok(v) => v,
            Err(e) => {
                println!("Failed to parse a slack json object: {:?}", e);
                return Ok(HttpResponse::build(StatusCode::OK)
                    .content_type("text/html; charset=utf-8")
                    .body(body));
            }
        };

        println!("Parsed Event: {:?}", posted_event);

        match posted_event {
            slack::SlackEvent::UrlVerification { challenge, .. } => {
                Ok(HttpResponse::build(StatusCode::OK)
                    .content_type("application/x-www-form-urlencoded")
                    .body(challenge.to_string()))
            }
            slack::SlackEvent::EventCallback(event_callback) => {
                match event_callback.event {
                    slack::InternalEvent::Message(message) => {
                        if let Ok(message) = MessageEvent::from_slack_event(&message) {
                            state.sender.do_send(message)
                        }
                    }
                    slack::InternalEvent::LinkShared(message) => {
                        if let Ok(message) = MessageEvent::from_slack_link_event(&message) {
                            state.sender.do_send(message)
                        }
                    }
                };
                Ok(HttpResponse::build(StatusCode::OK)
                    .content_type("text/html; charset=utf-8")
                    .body(body))
            }
        }
    } else {
        // response
        Ok(HttpResponse::build(StatusCode::BAD_REQUEST)
            .content_type("text/html; charset=utf-8")
            .body(body))
    }
}

fn main() -> std::io::Result<()> {
    let bot_token = match env::var("SLACK_BOT_TOKEN") {
        Ok(val) => val,
        Err(_e) => panic!("Bot token is not given."),
    };
    println!("{:?}", bot_token);

    let signing_secret = match env::var("SLACK_SIGNING_SECRET") {
        Ok(val) => val,
        Err(_e) => panic!("Signing secret is not given."),
    };

    let system = System::new("slack");

    let slack_event_actor = SlackEventActor {
        bot_token: bot_token.clone(),
        bot_id: "URS3HL8SD".to_string(), //TODO: remove hardcoded value
        slack_client: reqwest::Client::new(),
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
            println!("Trying to bind ssl.");
            let mut config = ServerConfig::new(NoClientAuth::new());
            let cert_file = &mut BufReader::new(File::open("PUBLIC_KEY.pem").unwrap());
            let key_file = &mut BufReader::new(File::open("PRIVATE_KEY.pem").unwrap());
            let cert_chain = certs(cert_file).unwrap();
            let mut keys = rsa_private_keys(key_file).unwrap();

            if keys.len() == 0 {
                panic!("Fails to load a private key file. Check it is formatted in RSA.")
            }
            config.set_single_cert(cert_chain, keys.remove(0)).unwrap();

            http_srv = http_srv.bind_rustls("0.0.0.0:14475", config).unwrap();
        } else {
            println!("Trying to bind http.");

            http_srv = http_srv.bind("0.0.0.0:8082").unwrap();
        }

        let srv = http_srv.run();

        let _ = tx.send(srv);

        println!("Server run start!");

        system.run()
    });

    let srv = rx.recv().unwrap();
    let main_sys = System::current();
    let ctrl_c_pressed = std::sync::atomic::AtomicBool::new(false);

    ctrlc::set_handler(move || {
        if ctrl_c_pressed.load(std::sync::atomic::Ordering::SeqCst) {
            println!("Force to stop program.");
            std::process::exit(1);
        }
        ctrl_c_pressed.store(true, std::sync::atomic::Ordering::SeqCst);
        println!("Try to stop HttpServer.");
        executor::block_on(srv.stop(true));
        main_sys.stop();
        println!("Stopped.");
    })
    .expect("Fail to set Ctrl-C handler.");

    system.run()
}

// curl 'https://slack.com/api/chat.postMessage' -H 'Authorization: Bearer SECRET' -H 'Content-type: application/json; charset=utf-8' -d '{"channel": "CS2AVF83X", "text": "hello, world"}'
