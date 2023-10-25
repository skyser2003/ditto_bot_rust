use crate::{slack, Message};

use reqwest::Url;

pub async fn handle<B: crate::Bot>(bot: &B, msg: &crate::MessageEvent) -> anyhow::Result<()> {
    if let Some(link) = &msg.link {
        let mut parsed_url = Url::parse(link)?;
        let url_string = parsed_url.host_str().unwrap_or_default();

        if url_string != "x.com" && url_string != "twitter.com" {
            return Ok(());
        }

        parsed_url.set_host(Some("vxtwitter.com"))?;

        bot.send_message(
            &msg.channel,
            Message::Blocks(&[slack::BlockElement::RichText {
                block_id: "".to_string(),
                elements: vec![slack::BlockElement::RichTextSection {
                    elements: vec![slack::BlockElement::Link(slack::LinkBlock {
                        url: parsed_url.to_string(),
                    })],
                }],
            }]),
            None,
            Some(true),
        )
        .await?;
    }
    Ok(())
}
