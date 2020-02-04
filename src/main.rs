extern crate slack;

mod message_handler;
mod slack_handler;

use std::env;
use slack::RtmClient;
use message_handler::NamuwikiHandler;
use slack_handler::{SlackHandler, SlackClientWrapper};
fn main() {
    let token = env::var("token").unwrap();
    let cli = RtmClient::login(&token).unwrap();

    let mut cli_wrapper = SlackClientWrapper{ cli: &cli };
    let mut handler = SlackHandler{channel_id: String::new(), cli: &mut cli_wrapper, handlers: Vec::new()};

    let namu_handler = NamuwikiHandler::default();
    handler.handlers.push(Box::new(namu_handler));

    let r = cli.run(&mut handler);

    match r {
        Ok(_) => {}
        Err(err) => panic!("Error: {}", err)
    }
}