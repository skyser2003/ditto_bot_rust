extern crate slack;

use slack::RtmClient;
use slack::api::MessageStandard;

use crate::message_handler::MessageHandler;

pub struct NamuwikiHandler;

impl MessageHandler for NamuwikiHandler {
    fn on_message(&mut self, cli: &RtmClient, msg: &MessageStandard) {
        let _ = cli.sender().send_message(msg.channel.as_ref().unwrap(), "안녕?");
    }
}