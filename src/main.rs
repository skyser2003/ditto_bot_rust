extern crate slack;

mod message_handler;
mod slack_handler;

use std::env;
use slack::RtmClient;
use message_handler::NamuwikiHandler;
use slack_handler::SlackHandler;

fn main() {
    let token = env::var("token").unwrap();
    let mut handler = SlackHandler{channel_id: String::new(), cli: None, handlers: Vec::new()};

    let namu_handler = NamuwikiHandler;
    handler.handlers.push(Box::new(namu_handler));

    let cli = RtmClient::login(&token).unwrap();
    handler.cli.replace(&cli);

    let r = cli.run(&mut handler);

    match r {
        Ok(_) => {}
        Err(err) => panic!("Error: {}", err)
    }
}