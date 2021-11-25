use crate::slack;
use lazy_static::lazy_static;
use regex::Regex;
use reqwest::Url;
use std::borrow::Cow;

lazy_static! {
    static ref TITLE_REGEX: Regex = Regex::new(r"<title>(.+) - 나무위키</title>").unwrap();
}

pub async fn handle<'a>(
    link_opt: Option<String>,
    blocks: &mut Vec<slack::BlockElement<'a>>,
) -> anyhow::Result<()> {
    if let Some(link) = link_opt {
        let parsed_url = Url::parse(&link)?;
        let url_string = parsed_url.host_str().unwrap_or_default();

        if url_string != "namu.wiki" {
            return Ok(());
        }

        let title = {
            let client = reqwest::Client::builder()
                .user_agent("Mozilla/5.0 (X11; Linux x86_64; rv:94.0) Gecko/20100101 Firefox/94.0")
                .build()?;
            let res = client.get(&link).send().await?;
            let body = res.text().await?;

            let title_opt = TITLE_REGEX
                .captures(&body)
                .and_then(|captures| captures.get(1).map(|match_title| match_title.as_str()));

            match title_opt {
                Some(val) => format!("{} - 나무위키", val),
                None => "Invalid url".to_string(),
            }
        };

        blocks.push(slack::BlockElement::Actions(slack::ActionBlock {
            block_id: None,
            elements: Some(vec![slack::BlockElement::Button(slack::ButtonBlock {
                text: slack::TextObject {
                    ty: slack::TextObjectType::PlainText,
                    text: Cow::from(title),
                    emoji: None,
                    verbatim: None,
                },
                action_id: None,
                url: Some(Cow::from(link)),
                value: None,
                style: Some(slack::ButtonStyle::Primary),
            })]),
        }));
    }
    Ok(())
}
