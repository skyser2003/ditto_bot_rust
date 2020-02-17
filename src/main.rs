use std::env;
use actix_web::{get, post, web, App, HttpServer, Responder};
use serde::Deserialize;

struct SlackApiBase {
    token: String,
    r#type: String
}
#[derive(Deserialize)]
struct SlackChallenge {
    token: String,
    r#type: String,
    challenge: String
}

#[get("/{id}/{name}/index.html")]
async fn index(info: web::Path<(u32, String)>) -> impl Responder {
    format!("Hello {}! id: {}", info.1, info.0)
}

#[post("/")]
async fn slack_challenge(body: web::Json<SlackChallenge>) -> impl Responder {
    body.challenge.clone()
}

#[actix_rt::main]
async fn main() -> std::io::Result<()> {
    let token = env::var("token").unwrap();

    let app = || App::new()
    .service(index)
    .service(slack_challenge);

    HttpServer::new(app)
    .bind("0.0.0.0:80")?
    .run()
    .await
}