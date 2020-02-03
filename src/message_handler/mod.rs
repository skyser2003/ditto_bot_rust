extern crate slack;

mod namuwiki_handler;
pub use namuwiki_handler::NamuwikiHandler;

use slack::api::MessageStandard;

use crate::slack_handler::SlackClientWrapper;

pub trait MessageHandler {
    fn on_message(&mut self, cli: &mut SlackClientWrapper, msg: &MessageStandard);
}