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
			"text": "",
			"unfurl_media": true,
			"unfurl_links": true,
			"attachments": [
				{{
					"fallback": "{}",
					"actions": [
						{{
							"type": "button",
							"text": "{}",
							"url": "{}",
							"style": "primary"
						}}
					]
				}}
			]
        }}"#, channel_id, link_text, link_text, link_url);

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