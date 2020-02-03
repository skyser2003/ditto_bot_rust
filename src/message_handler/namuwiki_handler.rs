extern crate slack;

use slack::api::MessageStandard;

use crate::message_handler::MessageHandler;
use crate::slack_handler::{SlackClientWrapper, SlackClientWrapperFunc};

pub struct NamuwikiHandler;

impl MessageHandler for NamuwikiHandler {
    fn on_message(&mut self, cli: &mut SlackClientWrapper, msg: &MessageStandard) {
        cli.send(msg.channel.as_ref().unwrap(), "안녕?");
    }
}