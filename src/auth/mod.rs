use std::{marker::PhantomData, sync::Arc};

use axum::{
    body::Body,
    body::BoxBody,
    http::{Request, Response, StatusCode},
};
use bytes::Buf;
use futures::future::BoxFuture;
use hmac::Mac;
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

#[derive(Debug)]
pub struct SlackAuthorization<BOut>(Arc<Vec<u8>>, PhantomData<BOut>);

impl<BOut> Clone for SlackAuthorization<BOut> {
    fn clone(&self) -> Self {
        Self(self.0.clone(), PhantomData)
    }
}

impl<BOut> SlackAuthorization<BOut> {
    pub fn new(secret: Vec<u8>) -> Self {
        Self(Arc::new(secret), PhantomData)
    }
}

pub trait FromBody {
    fn from_body(body: Body) -> Self;
}

impl FromBody for Body {
    fn from_body(body: Body) -> Self {
        body
    }
}

impl FromBody for BoxBody {
    fn from_body(body: Body) -> Self {
        axum::body::boxed(body)
    }
}

fn empty_response<BOut>(status_code: StatusCode) -> Response<BOut>
where
    BOut: FromBody,
{
    Response::builder()
        .status(status_code)
        .body(FromBody::from_body(Body::empty()))
        .unwrap_or_else(|_| unsafe { std::hint::unreachable_unchecked() })
}

async fn impl_authorize<BIn, BOut>(
    secret: Arc<Vec<u8>>,
    mut request: Request<BIn>,
) -> Result<Request<BOut>, Response<BOut>>
where
    BIn: axum::body::HttpBody + Unpin + Send + Sync + 'static,
    BOut: FromBody,
    <BIn as axum::body::HttpBody>::Error: std::fmt::Display,
{
    let mut mac = {
        let headers = request.headers();
        let timestamp = if let Some(t) = headers.get("X-Slack-Request-Timestamp") {
            t.to_str()
                .map_err(|_| empty_response(StatusCode::BAD_REQUEST))?
        } else {
            return Err(empty_response(StatusCode::BAD_REQUEST));
        };

        {
            let cur_timestamp = std::time::SystemTime::now()
                .duration_since(std::time::SystemTime::UNIX_EPOCH)
                .unwrap()
                .checked_sub(std::time::Duration::from_secs(
                    timestamp.parse::<u64>().unwrap(),
                ));

            debug!("now: {:?}", cur_timestamp.unwrap());
            //TODO: check replay attack
        }

        let mut mac = hmac::Hmac::<sha2::Sha256>::new_from_slice(AsRef::as_ref(secret.as_ref()))
            .unwrap_or_else(|_| unsafe { std::hint::unreachable_unchecked() });
        mac.update("v0:".as_bytes());
        mac.update(timestamp.as_bytes());
        mac.update(":".as_bytes());

        mac
    };

    let data = request.body_mut().data();
    let body = if let Some(chunk) = data.await {
        match chunk {
            Ok(chunk) => {
                let chunk = chunk.chunk();
                mac.update(chunk);
                Vec::from(chunk)
            }
            Err(e) => {
                error!("Failed to read http request body - {}", e);
                return Err(empty_response(StatusCode::BAD_REQUEST));
            }
        }
    } else {
        Vec::new()
    };

    let calculated_signature = format!("v0={:02x}", ByteBuf(&mac.finalize().into_bytes()));

    let headers = request.headers();
    let signature = if let Some(s) = headers.get("X-Slack-Signature") {
        s.to_str().unwrap()
    } else {
        return Err(empty_response(StatusCode::BAD_REQUEST));
    };

    if signature != calculated_signature {
        return Err(empty_response(StatusCode::BAD_REQUEST));
    }

    debug!("Success to verify a slack's signature.");

    let mut req = Request::new(FromBody::from_body(body.into()));
    std::mem::swap(req.headers_mut(), request.headers_mut());

    Ok(req)
}

type SlackAuthorizationFuture<BOut> = BoxFuture<'static, Result<Request<BOut>, Response<BOut>>>;

impl<BIn, BOut> tower_http::auth::AsyncAuthorizeRequest<BIn> for SlackAuthorization<BOut>
where
    BIn: axum::body::HttpBody + Unpin + Send + Sync + 'static,
    BOut: FromBody,
    <BIn as axum::body::HttpBody>::Error: std::fmt::Display,
{
    type RequestBody = BOut;
    type ResponseBody = BOut;
    type Future = SlackAuthorizationFuture<BOut>;

    fn authorize(&mut self, request: Request<BIn>) -> Self::Future {
        let secret = self.0.clone();
        Box::pin(async move { impl_authorize::<BIn, BOut>(secret, request).await })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::HttpBody;
    use tower::{BoxError, Service, ServiceBuilder, ServiceExt};
    use tower_http::auth::AsyncRequireAuthorizationLayer;

    #[tokio::test]
    async fn slack_authorization_works() {
        const SECRET: &'static [u8] = b"8f742231b10e8888abcd99yyyzzz85a5";
        const SIGNATURE: &'static str =
            "v0=a2114d57b48eac39b9ad189dd8316235a7b4a8d21a10bd27519666489c69b503";
        const TIMESTAMP: &'static str = "1531420618";
        const BODY: &'static str = include_str!("test_body");

        let mut service = ServiceBuilder::new()
            .layer(AsyncRequireAuthorizationLayer::new(
                SlackAuthorization::new(SECRET.iter().cloned().collect()),
            ))
            .service_fn(echo);

        let service = ServiceExt::<Request<Body>>::ready(&mut service)
            .await
            .unwrap();

        {
            // correct signature/timestamp
            let request = Request::get("/")
                .header("X-Slack-Signature", SIGNATURE)
                .header("X-Slack-Request-Timestamp", TIMESTAMP)
                .header("Content-Type", "plain/text")
                .body(Body::from(BODY))
                .unwrap();

            let mut res = service.call(request).await.unwrap();

            assert_eq!(res.status(), StatusCode::OK);
            assert_eq!(
                res.headers().get("Content-Type").unwrap().to_str().unwrap(),
                "plain/text"
            );

            let body = res.body_mut().data().await.unwrap().unwrap();
            let body = body.chunk();
            let res_body = std::str::from_utf8(body).unwrap();
            assert_eq!(res_body, BODY);
        }

        {
            // missing signature
            let request = Request::get("/")
                .header("X-Slack-Request-Timestamp", TIMESTAMP)
                .body(Body::from(BODY))
                .unwrap();

            let res = service.call(request).await.unwrap();

            assert_eq!(res.status(), StatusCode::BAD_REQUEST);
        }

        {
            // invalid signature
            let request = Request::get("/")
                .header(
                    "X-Slack-Signature",
                    "v0=a2114d57b48eac39b9ad189dd8316235a7b4a8d21a10bd27519666489c69b502",
                )
                .header("X-Slack-Request-Timestamp", TIMESTAMP)
                .body(Body::from(BODY))
                .unwrap();

            let res = service.call(request).await.unwrap();

            assert_eq!(res.status(), StatusCode::BAD_REQUEST);
        }
    }

    async fn echo(mut req: Request<Body>) -> Result<Response<Body>, BoxError> {
        let body = Vec::from(req.body_mut().data().await.unwrap().unwrap().chunk());
        let mut res = Response::new(body.into());
        std::mem::swap(res.headers_mut(), req.headers_mut());
        Ok(res)
    }
}
