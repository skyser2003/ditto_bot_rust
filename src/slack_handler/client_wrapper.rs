extern crate slack;

use slack::RtmClient;

pub struct SlackClientWrapper<'a> {
    pub cli: &'a RtmClient
}

pub trait SlackClientWrapperFunc {
    fn send(&mut self, channel_id: &str, msg: &str);
}

impl<'a> SlackClientWrapperFunc for SlackClientWrapper<'a> {
    fn send(&mut self, channel_id: &str, msg: &str) {
        let _ = self.cli.sender().send_message(&channel_id, &msg);
    }
}