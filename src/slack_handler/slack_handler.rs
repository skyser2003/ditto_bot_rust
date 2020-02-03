extern crate slack;

use slack::{Event, RtmClient, Channel, Message};
use crate::message_handler::MessageHandler;
use crate::slack_handler::{SlackClientWrapper, SlackClientWrapperFunc};

pub struct SlackHandler<'a> {
    pub channel_id: String,
    pub cli: &'a mut SlackClientWrapper<'a>,
    pub handlers: Vec<Box<dyn MessageHandler>>
}

#[allow(unused_variables)]
impl<'a> slack::EventHandler for SlackHandler<'a> {
    fn on_event(&mut self, cli: &RtmClient, event: Event) {
         println!("on_event(event: {:?})", event);

         match event {
             Event::Hello => {
                 self.cli.send(&self.channel_id, "Hello World! (rtm)");
             }
             Event::Message(message) => match *message {
                 Message::Standard(msg) => {
                     for handler in &mut self.handlers {
                         handler.on_message(&mut *self.cli, &msg);
                     }

                     let user_id = msg.user.expect("");
                     if user_id == "some_id" {
                         // let _ = cli.sender().send_message(&mut self.channel_id, "<@some_id> 맞춤법 지키세요!");
                     }
                 }
                 _ => {}
             }
             _ => {}
         }
    }

    fn on_connect(&mut self, cli: &RtmClient) {
        println!("on_connect");

        let channel_id = cli.start_response()
        .channels
        .as_ref()
        .and_then(|channels| {
            channels
            .iter()
            .find(|chan: &&Channel| match chan.name {
                None => false,
                Some(ref name) => name == "random",
            })
        })
        .and_then(|chan| chan.id.as_ref())
        .expect("random channel not found");

        self.channel_id = channel_id.clone();
    }

    fn on_close(&mut self, cli: &RtmClient) {
        println!("on_close");
    }
}