use std::env;
use actix_web::{get, web, App, HttpServer, Responder};
use serde::Deserialize;

#[derive(Deserialize)]
struct SlackChallenge {
    challenge: String
}

#[get("/{id}/{name}/index.html")]
async fn index(info: web::Path<(u32, String)>) -> impl Responder {
    format!("Hello {}! id: {}", info.1, info.0)
}

async fn slack_challenge(web::Query(query): web::Query<SlackChallenge>) -> impl Responder {
    query.challenge
}

#[actix_rt::main]
async fn main() -> std::io::Result<()> {
    let token = env::var("token").unwrap();
    
    let app = || App::new()
    .service(index)
    .service(web::resource("/").route(web::get().to(slack_challenge)));

    HttpServer::new(app)
    .bind("0.0.0.0:80")?
    .run()
    .await
}