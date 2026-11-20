use std::sync::Arc;

use axum::{
    body::Body,
    http::{Request, Response, StatusCode},
    response::IntoResponse,
};
use futures::{future::BoxFuture, StreamExt};
use hmac::{KeyInit, Mac};
use log::{debug, error};

struct ByteBuf<'a>(&'a [u8]);

impl<'a> std::fmt::LowerHex for ByteBuf<'a> {
    fn fmt(&self, fmtr: &mut std::fmt::Formatter) -> std::result::Result<(), std::fmt::Error> {
        for byte in self.0 {
            fmtr.write_fmt(format_args!("{:02x}", byte))?;
        }
        Ok(())
    }
}

#[derive(Clone, Debug)]
pub(super) struct SlackAuthorization(Arc<Vec<u8>>);

impl SlackAuthorization {
    pub(super) fn new(secret: Vec<u8>) -> Self {
        Self(Arc::new(secret))
    }

    async fn authorize_request(&self, request: Request<Body>) -> Result<Request<Body>, StatusCode> {
        let (parts, body) = request.into_parts();
        let timestamp = parts
            .headers
            .get("X-Slack-Request-Timestamp")
            .and_then(|timestamp| timestamp.to_str().ok())
            .ok_or(StatusCode::BAD_REQUEST)?;

        let cur_timestamp = std::time::SystemTime::now()
            .duration_since(std::time::SystemTime::UNIX_EPOCH)
            .unwrap()
            .checked_sub(std::time::Duration::from_secs(
                timestamp.parse::<u64>().unwrap(),
            ));
        debug!("now: {:?}", cur_timestamp.unwrap());
        //TODO: check replay attack

        let mut mac = hmac::Hmac::<sha2::Sha256>::new_from_slice(self.0.as_slice())
            .expect("HMAC accepts keys of any length");
        mac.update("v0:".as_bytes());
        mac.update(timestamp.as_bytes());
        mac.update(":".as_bytes());

        let bytes = body
            .into_data_stream()
            .next()
            .await
            .transpose()
            .map_err(|error| {
                error!("Failed to read http request body - {}", error);
                StatusCode::BAD_REQUEST
            })?
            .unwrap_or_default();
        mac.update(&bytes);

        let calculated_signature = format!("v0={:02x}", ByteBuf(&mac.finalize().into_bytes()));
        let signature = parts
            .headers
            .get("X-Slack-Signature")
            .ok_or(StatusCode::BAD_REQUEST)?
            .to_str()
            .unwrap();
        if signature != calculated_signature {
            return Err(StatusCode::BAD_REQUEST);
        }

        debug!("Success to verify a slack's signature.");
        Ok(Request::from_parts(parts, Body::from(bytes)))
    }
}

impl tower_http::auth::AsyncAuthorizeRequest<Body> for SlackAuthorization {
    type RequestBody = Body;
    type ResponseBody = Body;
    type Future = BoxFuture<'static, Result<Request<Body>, Response<Body>>>;

    fn authorize(&mut self, request: Request<Body>) -> Self::Future {
        let authorization = self.clone();
        Box::pin(async move {
            authorization
                .authorize_request(request)
                .await
                .map_err(IntoResponse::into_response)
        })
    }
}

#[cfg(test)]
mod tests;
