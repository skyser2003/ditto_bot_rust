use slack::api::MessageStandard;
use regex::Regex;
use url::{Url, Host, Position};

use crate::message_handler::MessageHandler;
use crate::slack_handler::{SlackClientWrapper, SlackClientWrapperFunc};

pub struct NamuwikiHandler {
    title_regex: Regex
}

impl Default for NamuwikiHandler {
    fn default() -> Self {
        NamuwikiHandler{
             title_regex: Regex::new(r"").unwrap()
        }
    }
}

impl MessageHandler for NamuwikiHandler {
    fn on_message(&mut self, cli: &mut SlackClientWrapper, msg: &MessageStandard) {
        let raw_text = msg.text.as_ref().unwrap();

        if raw_text.chars().next().unwrap() != '<' || raw_text.chars().last().unwrap() != '>' {
            return
        }

        let text = &raw_text[1..raw_text.len() - 1];
        
        let parse_result = Url::parse(text);

        match parse_result {
            Ok(parsed_url) => {
                if parsed_url.host_str().unwrap() == "namu.wiki" {
                    
                }
            }
            Err(_) => {} // Not a URL
        }

        // cli.send(msg.channel.as_ref().unwrap(), "안녕?");
    }
}