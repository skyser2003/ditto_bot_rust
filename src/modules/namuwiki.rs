use crate::{slack, Message};

use once_cell::sync::OnceCell;
use regex::Regex;
use reqwest::Url;

static TITLE_REGEX: OnceCell<Regex> = OnceCell::new();

pub async fn handle<B: crate::Bot>(bot: &B, msg: &crate::MessageEvent) -> anyhow::Result<()> {
    if let Some(link) = &msg.link {
        let parsed_url = Url::parse(link)?;
        let url_string = parsed_url.host_str().unwrap_or_default();

        if url_string != "namu.wiki" {
            return Ok(());
        }

        let title = {
            let client = reqwest::Client::builder()
                .user_agent("Mozilla/5.0 (X11; Linux x86_64; rv:94.0) Gecko/20100101 Firefox/94.0")
                .build()?;
            let res = client.get(link).send().await?;
            let body = res.text().await?;

            let title_opt = TITLE_REGEX
                .get_or_try_init(|| Regex::new(r"<title>(.+) - 나무위키</title>"))?
                .captures(&body)
                .and_then(|captures| captures.get(1).map(|match_title| match_title.as_str()));

            get_parsed_title(title_opt, &parsed_url)
        };

        bot.send_message(
            &msg.channel,
            Message::Blocks(&[slack::BlockElement::Actions(slack::ActionBlock {
                block_id: None,
                elements: Some(vec![slack::BlockElement::Button(slack::ButtonBlock {
                    text: slack::TextObject {
                        ty: slack::TextObjectType::PlainText,
                        text: title.to_string(),
                        emoji: None,
                        verbatim: None,
                    },
                    action_id: None,
                    url: Some(link.to_string()),
                    value: None,
                    style: Some(slack::ButtonStyle::Primary),
                })]),
            })]),
            None,
            None,
        )
        .await?;
    }
    Ok(())
}

fn get_parsed_title(title_opt: Option<&str>, parsed_url: &Url) -> String {
    let title_text = match title_opt {
        Some(val) => val.to_string(),
        None => {
            let fragments = parsed_url.path_segments();

            let mut fallback_title = Option::<String>::None;

            if let Some(mut fragments) = fragments {
                let wpart = fragments.next().unwrap_or_default();

                if wpart == "w" {
                    let rest_parts = fragments.collect::<Vec<_>>();

                    let decoded = rest_parts
                        .iter()
                        .map(|part| {
                            percent_encoding::percent_decode_str(part)
                                .decode_utf8_lossy()
                                .to_string()
                        })
                        .collect::<Vec<_>>()
                        .join("/");

                    fallback_title = Some(decoded);
                }
            }

            match fallback_title {
                Some(val) => val,
                None => "Invalid url".to_string(),
            }
        }
    };

    format!("{} - 나무위키", title_text)
}

#[tokio::test]
#[cfg(test)]
async fn test_url_fallback() -> anyhow::Result<()> {
    let link = "https://namu.wiki/w/Pok%C3%A9mon%20Sleep/%EC%9A%94%EB%A6%AC";
    let invalid_link = "https://namu.wiki";
    let parsed_url = Url::parse(link)?;
    let failed_url = Url::parse(invalid_link)?;

    let parse_success = get_parsed_title(Some("Pikachu"), &parsed_url);
    let parse_failed = get_parsed_title(None, &parsed_url);
    let all_failed = get_parsed_title(None, &failed_url);

    assert_eq!(parse_success, "Pikachu - 나무위키");
    assert_eq!(parse_failed, "Pokémon Sleep/요리 - 나무위키");
    assert_eq!(all_failed, "Invalid url - 나무위키");

    Ok(())
}
