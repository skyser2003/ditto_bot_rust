use std::env;
use futures::{Stream, StreamExt};

use actix_web::{get, post, web, App, HttpServer, Responder, HttpResponse, Error};
use serde::Deserialize;

#[derive(Deserialize)]
struct MesssageEvent {
    r#type: String,
    channel: String,
    user: String,
    text: String,
    ts: String,
    event_ts: String,
    channel_type: String
}

#[derive(Deserialize)]
struct SlackChallenge {
    token: String,
    r#type: String,
    challenge: String
}

#[derive(Deserialize)]
struct SlackMessage {
    token: String,
    r#type: String,
    team_id: String,
    api_app_id: String,
    event: MesssageEvent,
    authed_teams: Vec<String>,
    event_id: String,
    event_time: i32
}

#[get("/{id}/{name}/index.html")]
async fn index(info: web::Path<(u32, String)>) -> impl Responder {
    format!("Hello {}! id: {}", info.1, info.0)
}

#[post("/")]
async fn slack_challenge(body: web::Json<SlackChallenge>) -> impl Responder {
    body.challenge.clone()
}

#[post("/")]
async fn slack_api_response(mut body: web::Payload) -> Result<HttpResponse, Error> {
    let mut bytes = web::BytesMut::new();

    while let Some(item) = body.next().await {
        bytes.extend_from_slice(&item?);
    }

    let body_str = std::str::from_utf8(&bytes.to_vec())?.to_string();
    println!("Body {}", body_str);

    let parsed_raw: serde_json::Value = serde_json::from_str(&body_str)?;

    match parsed_raw["type"].as_str().unwrap() {
        "url_verification" => {
            let msg: SlackChallenge = serde_json::from_str(&body_str)?;
            Ok(HttpResponse::Ok().body(msg.challenge.clone()))
        }
        "message.channels" => {
            let msg: SlackMessage = serde_json::from_str(&body_str)?;
            Ok(HttpResponse::Ok().body("booboo"))
        }
        &_ => Ok(HttpResponse::Ok().finish())
    }
}

#[actix_rt::main]
async fn main() -> std::io::Result<()> {
    let token = env::var("token").unwrap();

    let app = || App::new()
    .service(index)
//    .service(slack_challenge)
    .service(slack_api_response);

    HttpServer::new(app)
    .bind("0.0.0.0:80")?
    .run()
    .await
}