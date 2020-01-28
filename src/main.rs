extern crate slack;
use slack::{Event, RtmClient, Channel, Message};
use std::env;

struct MyHandler<'a> {
    channel_id: String,
    cli: Option<&'a RtmClient>
}

trait MyHandlerFunc {
    fn send(&mut self, msg: &str);
}

#[allow(unused_variables)]
impl<'a> slack::EventHandler for MyHandler<'a> {
    fn on_event(&mut self, cli: &RtmClient, event: Event) {
         println!("on_event(event: {:?})", event);

         match event {
             Event::Hello => {
                 self.send("Hello World! (rtm)");
             }
             Event::Message(message) => match *message {
                 Message::Standard(msg) => {
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

        self.channel_id.push_str(channel_id);
    }

    fn on_close(&mut self, cli: &RtmClient) {
        println!("on_close");
    }
}

impl<'a> MyHandlerFunc for MyHandler<'a> {
    fn send(&mut self, msg: &str) {
        let _ = self.cli.unwrap().sender().send_message(&mut self.channel_id, msg);
    }
}

fn main() {
    let token = env::var("token").unwrap();
    let mut handler = MyHandler{channel_id: String::new(), cli: None};

    let cli = RtmClient::login(&token).unwrap();
    handler.cli.replace(&cli);

    let r = cli.run(&mut handler);

    match r {
        Ok(_) => {}
        Err(err) => panic!("Error: {}", err)
    }
}