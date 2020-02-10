use std::env;
use actix_web::{get, web, App, HttpServer, Responder};

#[get("/{id}/{name}/index.html")]
async fn index(info: web::Path<(u32, String)>) -> impl Responder {
    format!("Hello {}! id: {}", info.1, info.0)
}

#[actix_rt::main]
async fn main() -> std::io::Result<()> {
    let token = env::var("token").unwrap();

    HttpServer::new(|| App::new().service(index))
    .bind("0.0.0.0:80")?
    .run()
    .await
}