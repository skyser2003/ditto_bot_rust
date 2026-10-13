use std::time::Duration;

use anyhow::{bail, Context};
use bytes::{Bytes, BytesMut};
use futures::StreamExt;

use super::SlackFile;

#[cfg(test)]
mod test;

pub const MAX_IMAGE_BYTES: u64 = 20 * 1024 * 1024;

fn download_request(
    client: &reqwest::Client,
    token: &str,
    file: &SlackFile,
) -> anyhow::Result<reqwest::RequestBuilder> {
    let url = file
        .url_private_download
        .as_deref()
        .or(file.url_private.as_deref())
        .context("Image has no download URL")?;
    let url = reqwest::Url::parse(url).context("Invalid Slack file URL")?;
    if url.scheme() != "https"
        || url.host_str() != Some("files.slack.com")
        || url.port_or_known_default() != Some(443)
        || !url.username().is_empty()
        || url.password().is_some()
    {
        bail!("Image download URL must be hosted on files.slack.com over HTTPS");
    }
    if file.size.unwrap_or_default() > MAX_IMAGE_BYTES {
        bail!("Image exceeds the 20 MiB limit");
    }

    Ok(client
        .get(url)
        .bearer_auth(token)
        .timeout(Duration::from_secs(30)))
}

pub async fn download_file(
    client: &reqwest::Client,
    token: &str,
    file: &SlackFile,
) -> anyhow::Result<Bytes> {
    let response = download_request(client, token, file)?
        .send()
        .await
        .map_err(reqwest::Error::without_url)
        .context("Failed to download image from Slack")?;
    read_image_response(response).await
}

async fn read_image_response(response: reqwest::Response) -> anyhow::Result<Bytes> {
    let response = response
        .error_for_status()
        .map_err(reqwest::Error::without_url)
        .context("Slack rejected the image download; check the bot's files:read scope")?;
    if response.content_length().unwrap_or_default() > MAX_IMAGE_BYTES {
        bail!("Image exceeds the 20 MiB limit");
    }
    if let Some(content_type) = response.headers().get(reqwest::header::CONTENT_TYPE) {
        let content_type = content_type
            .to_str()?
            .split(';')
            .next()
            .unwrap_or("")
            .trim();
        if !content_type.starts_with("image/") && content_type != "application/octet-stream" {
            bail!("Slack did not return an image; check the bot's files:read scope");
        }
    }

    let mut body = BytesMut::new();
    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk
            .map_err(reqwest::Error::without_url)
            .context("Failed to read Slack image")?;
        if body.len() as u64 + chunk.len() as u64 > MAX_IMAGE_BYTES {
            bail!("Image exceeds the 20 MiB limit");
        }
        body.extend_from_slice(&chunk);
    }
    if body.is_empty() {
        bail!("Slack returned an empty image");
    }
    Ok(body.freeze())
}
