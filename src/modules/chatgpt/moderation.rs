use std::{
    collections::{BTreeMap, BTreeSet},
    time::Duration,
};

use anyhow::{ensure, Context};
use log::error;
use serde::{Deserialize, Serialize};

use crate::{
    slack::{BlockElement, SectionBlock},
    Bot, Message, MessageEvent, ReplyMessageEvent,
};

#[cfg(test)]
mod test;

const MODERATION_URL: &str = "https://api.openai.com/v1/moderations";

#[derive(Serialize)]
struct ModerationRequest<'a> {
    model: &'static str,
    input: &'a str,
}

#[derive(Deserialize)]
struct ModerationResponse {
    results: Vec<ModerationResult>,
}

#[derive(Deserialize)]
struct ModerationResult {
    categories: BTreeMap<String, bool>,
}

pub(super) fn parse_command<'a>(text: &'a str, mention: &str) -> Option<&'a str> {
    let command = text.trim_start().strip_prefix(mention)?.trim_start();
    let input = command.strip_prefix("mod")?;
    if input.is_empty() {
        Some(input)
    } else {
        input.strip_prefix(char::is_whitespace)
    }
}

fn request(client: &reqwest::Client, url: &str, key: &str, input: &str) -> reqwest::RequestBuilder {
    client
        .post(url)
        .bearer_auth(key)
        .timeout(Duration::from_secs(30))
        .json(&ModerationRequest {
            model: "omni-moderation-latest",
            input,
        })
}

pub(super) async fn handle<B: Bot>(bot: &B, msg: &MessageEvent, input: &str) -> anyhow::Result<()> {
    let request = request(
        &reqwest::Client::new(),
        MODERATION_URL,
        bot.openai_key(),
        input,
    );
    handle_request(bot, msg, input, request).await
}

async fn handle_request<B: Bot>(
    bot: &B,
    msg: &MessageEvent,
    input: &str,
    request: reqwest::RequestBuilder,
) -> anyhow::Result<()> {
    let text = if input.trim().is_empty() {
        "Usage: mention the bot followed by mod <text>.".to_string()
    } else {
        match flagged_categories(request).await {
            Ok(categories) if categories.is_empty() => "No flagged categories.".to_string(),
            Ok(categories) => format!(
                "Flagged categories:\n{}",
                categories
                    .into_iter()
                    .map(|category| format!("• {}", category))
                    .collect::<Vec<_>>()
                    .join("\n")
            ),
            Err(error) => {
                error!("OpenAI moderation request failed: {}", error);
                "OpenAI moderation request failed. Please try again.".to_string()
            }
        }
    };
    let blocks = [
        BlockElement::Section(SectionBlock::new_markdown("`Moderation`")),
        BlockElement::Section(SectionBlock::new_text(&text)),
    ];
    let reply = Some(ReplyMessageEvent {
        msg: msg.thread_ts.clone().unwrap_or_else(|| msg.ts.clone()),
        broadcast: msg.reply_broadcast,
    });
    bot.send_message(&msg.channel, Message::Blocks(&blocks), reply, None)
        .await?;
    Ok(())
}

async fn flagged_categories(request: reqwest::RequestBuilder) -> anyhow::Result<BTreeSet<String>> {
    let response = request
        .send()
        .await
        .map_err(reqwest::Error::without_url)
        .context("Failed to reach the OpenAI moderation API")?
        .error_for_status()
        .map_err(reqwest::Error::without_url)
        .context("OpenAI moderation returned an unsuccessful HTTP status")?;
    let response: ModerationResponse = response
        .json()
        .await
        .map_err(reqwest::Error::without_url)
        .context("OpenAI moderation returned an invalid response")?;
    ensure!(
        !response.results.is_empty(),
        "OpenAI moderation returned no results"
    );
    let mut flagged = BTreeSet::new();
    for result in response.results {
        ensure!(
            !result.categories.is_empty(),
            "OpenAI moderation returned no categories"
        );
        flagged.extend(
            result
                .categories
                .into_iter()
                .filter_map(|(category, flagged)| flagged.then_some(category)),
        );
    }
    Ok(flagged)
}
