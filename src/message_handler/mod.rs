extern crate slack;

mod namuwiki_handler;

pub use namuwiki_handler::NamuwikiHandler;

use slack::api::MessageStandard;

pub trait MessageHandler {
    fn on_message(&mut self, msg: &MessageStandard);
}