extern crate slack;

use crate::message_handler::MessageHandler;
use slack::api::MessageStandard;

pub struct NamuwikiHandler;

impl MessageHandler for NamuwikiHandler {
    fn on_message(&mut self, msg: &MessageStandard) {

    }
}