use super::*;
use reqwest::{header, StatusCode};
use serde_json::json;

fn file(url: Option<&str>) -> SlackFile {
    serde_json::from_value(json!({
        "id": "F123", "mimetype": "image/png", "url_private": url
    }))
    .unwrap()
}

#[test]
fn private_file_request_uses_slack_authentication() {
    let client = reqwest::Client::new();
    let request = download_request(
        &client,
        "test-slack-token",
        &file(Some("https://files.slack.com/image.png")),
    )
    .unwrap()
    .build()
    .unwrap();
    assert_eq!(
        request.headers()[header::AUTHORIZATION],
        "Bearer test-slack-token"
    );
    assert_eq!(request.url().as_str(), "https://files.slack.com/image.png");
}

#[test]
fn private_download_url_is_preferred() {
    let client = reqwest::Client::new();
    let mut file = file(Some("https://files.slack.com/original.png"));
    file.url_private_download = Some("https://files.slack.com/download.png".to_string());
    let request = download_request(&client, "token", &file)
        .unwrap()
        .build()
        .unwrap();
    assert_eq!(request.url().path(), "/download.png");
}

#[test]
fn file_request_rejects_untrusted_or_missing_urls() {
    let client = reqwest::Client::new();
    for url in [
        None,
        Some("http://files.slack.com/image.png"),
        Some("https://example.com/image.png"),
        Some("https://files.slack.com.example.com/image.png"),
        Some("https://files.slack.com:8443/image.png"),
        Some("https://user@files.slack.com/image.png"),
    ] {
        assert!(
            download_request(&client, "token", &file(url)).is_err(),
            "URL must be rejected: {:?}",
            url
        );
    }
}

#[test]
fn oversized_file_metadata_is_rejected() {
    let client = reqwest::Client::new();
    let mut file = file(Some("https://files.slack.com/image.png"));
    file.size = Some(MAX_IMAGE_BYTES + 1);
    assert!(download_request(&client, "token", &file).is_err());
}

fn response(status: StatusCode, content_type: &str, body: reqwest::Body) -> reqwest::Response {
    axum::http::Response::builder()
        .status(status)
        .header(header::CONTENT_TYPE, content_type)
        .body(body)
        .unwrap()
        .into()
}

#[tokio::test]
async fn image_response_returns_original_bytes() {
    let response = response(StatusCode::OK, "image/png", b"image".to_vec().into());
    assert_eq!(read_image_response(response).await.unwrap(), b"image"[..]);
}

#[tokio::test]
async fn failed_or_html_download_is_rejected() {
    for (status, content_type) in [
        (StatusCode::FORBIDDEN, "image/png"),
        (StatusCode::OK, "text/html; charset=utf-8"),
    ] {
        assert!(
            read_image_response(response(status, content_type, "error".into()))
                .await
                .is_err()
        );
    }
}

#[tokio::test]
async fn empty_image_is_rejected() {
    assert!(
        read_image_response(response(StatusCode::OK, "image/png", Vec::new().into()))
            .await
            .is_err()
    );
}

#[tokio::test]
async fn oversized_response_is_rejected() {
    let body = vec![0; MAX_IMAGE_BYTES as usize + 1];
    assert!(
        read_image_response(response(StatusCode::OK, "image/png", body.into()))
            .await
            .is_err()
    );
}

#[tokio::test]
async fn chunked_response_is_limited_without_content_length() {
    let chunk = Bytes::from(vec![0; MAX_IMAGE_BYTES as usize / 2 + 1]);
    let chunks = futures::stream::iter(vec![Ok::<_, std::io::Error>(chunk.clone()), Ok(chunk)]);
    let body = reqwest::Body::wrap_stream(chunks);
    let response = response(StatusCode::OK, "application/octet-stream", body);
    assert_eq!(response.content_length(), None);
    assert!(read_image_response(response).await.is_err());
}

#[tokio::test]
async fn broken_image_stream_is_rejected() {
    let chunks = futures::stream::iter(vec![Err::<Bytes, _>(std::io::Error::new(
        std::io::ErrorKind::UnexpectedEof,
        "interrupted",
    ))]);
    let response = response(
        StatusCode::OK,
        "image/png",
        reqwest::Body::wrap_stream(chunks),
    );
    assert!(read_image_response(response).await.is_err());
}
