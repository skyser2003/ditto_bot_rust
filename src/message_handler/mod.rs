extern crate slack;

mod namuwiki_handler;

use slack::RtmClient;
use slack::api::MessageStandard;

pub use namuwiki_handler::NamuwikiHandler;

pub trait MessageHandler {
    fn on_message(&mut self, cli: &RtmClient, msg: &MessageStandard);
}