use crate::{slack, Message, ReplyMessageEvent};

use reqwest::Url;

pub async fn handle<B: crate::Bot>(bot: &B, msg: &crate::MessageEvent) -> anyhow::Result<()> {
    if let Some(link) = &msg.link {
        let mut parsed_url = Url::parse(link)?;
        let url_string = parsed_url.host_str().unwrap_or_default();

        if url_string != "x.com" && url_string != "twitter.com" {
            return Ok(());
        }

        parsed_url.set_host(Some("vxtwitter.com"))?;

        let reply_event = if let Some(thread_ts) = &msg.thread_ts {
            Some(ReplyMessageEvent {
                msg: thread_ts.to_string(),
                broadcast: false,
            })
        } else {
            None
        };

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
            reply_event,
            Some(true),
        )
        .await?;
    }
    Ok(())
}
