use slack::RtmClient;

pub struct SlackClientWrapper<'a> {
    pub cli: &'a RtmClient
}

pub trait SlackClientWrapperFunc {
    fn send_link(&mut self, channel_id: &str, link_text: &str, link_url: &str);
    fn send_message(&mut self, channel_id: &str, msg: &str);
}

impl<'a> SlackClientWrapperFunc for SlackClientWrapper<'a> {
    fn send_link(&mut self, channel_id: &str, link_text: &str, link_url: &str) {
        let raw_json = format!(r#"{{
            "channel": "{}",
			"blocks": [
                {{
                    "type": "actions",
                    "elements": [
                        {{
                            "type": "button",
                            "text": {{
                                "type": "plain_text",
                                "text": "Farmhouse",
                                "emoji": true
                            }},
                            "value": "click_me_123"
                        }}
                    ]
                }}
            ]
        }}"#, channel_id);

        println!("{}", raw_json);

        let r = self.cli.sender().send(&raw_json);

        match r {
            Ok(_) => {
                println!("Send successful");
            },
            Err(e) => {
                println!("{}", e);
            }
        }
    }

    fn send_message(&mut self, channel_id: &str, msg: &str) {
        let _ = self.cli.sender().send_message(&channel_id, &msg);
    }
}