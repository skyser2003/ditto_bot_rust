use std::time::Duration;

use anyhow::{bail, Context};
use serde::{Deserialize, Serialize};

#[cfg(test)]
mod test;

#[derive(Serialize)]
struct Reaction<'a> {
    channel: &'a str,
    timestamp: &'a str,
    name: &'a str,
}

#[derive(Deserialize)]
struct ReactionResponse {
    ok: bool,
    error: Option<String>,
}

fn reaction_request(
    client: &reqwest::Client,
    url: &str,
    token: &str,
    channel: &str,
    ts: &str,
    name: &str,
) -> reqwest::RequestBuilder {
    client
        .post(url)
        .bearer_auth(token)
        .timeout(Duration::from_secs(5))
        .json(&Reaction {
            channel,
            timestamp: ts,
            name,
        })
}

pub async fn add_reaction(
    client: &reqwest::Client,
    token: &str,
    channel: &str,
    ts: &str,
    name: &str,
) -> anyhow::Result<()> {
    send_reaction(
        reaction_request(
            client,
            "https://slack.com/api/reactions.add",
            token,
            channel,
            ts,
            name,
        ),
        "already_reacted",
    )
    .await
}

pub async fn remove_reaction(
    client: &reqwest::Client,
    token: &str,
    channel: &str,
    ts: &str,
    name: &str,
) -> anyhow::Result<()> {
    send_reaction(
        reaction_request(
            client,
            "https://slack.com/api/reactions.remove",
            token,
            channel,
            ts,
            name,
        ),
        "no_reaction",
    )
    .await
}

async fn send_reaction(
    request: reqwest::RequestBuilder,
    ignored_error: &str,
) -> anyhow::Result<()> {
    let response = request
        .send()
        .await
        .map_err(reqwest::Error::without_url)
        .context("Failed to send Slack reaction request")?;
    read_reaction_response(response, ignored_error).await
}

async fn read_reaction_response(
    response: reqwest::Response,
    ignored_error: &str,
) -> anyhow::Result<()> {
    let response: ReactionResponse = response
        .error_for_status()
        .map_err(reqwest::Error::without_url)
        .context("Slack reaction returned an unsuccessful HTTP status")?
        .json()
        .await
        .map_err(reqwest::Error::without_url)
        .context("Slack reaction returned an invalid response")?;
    if response.ok || response.error.as_deref() == Some(ignored_error) {
        return Ok(());
    }
    bail!(
        "Slack reaction failed: {}",
        response.error.as_deref().unwrap_or("unknown_error")
    )
}
