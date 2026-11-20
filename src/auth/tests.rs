use super::*;
use axum::body::to_bytes;
use tower::{BoxError, Service, ServiceBuilder, ServiceExt};
use tower_http::auth::AsyncRequireAuthorizationLayer;

#[tokio::test]
async fn slack_authorization_works() {
    const SECRET: &[u8] = b"8f742231b10e8888abcd99yyyzzz85a5";
    const SIGNATURE: &str = "v0=a2114d57b48eac39b9ad189dd8316235a7b4a8d21a10bd27519666489c69b503";
    const TIMESTAMP: &str = "1531420618";
    const BODY: &str = include_str!("test_body");

    let mut service = ServiceBuilder::new()
        .layer(AsyncRequireAuthorizationLayer::new(
            SlackAuthorization::new(SECRET.to_vec()),
        ))
        .service(tower::service_fn(echo));

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

        let res = service.call(request).await.unwrap();

        assert_eq!(res.status(), StatusCode::OK);
        assert_eq!(
            res.headers().get("Content-Type").unwrap().to_str().unwrap(),
            "plain/text"
        );

        let body = to_bytes(res.into_body(), BODY.len()).await.unwrap();
        assert_eq!(body.as_ref(), BODY.as_bytes());
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

async fn echo(req: Request<Body>) -> Result<Response<Body>, BoxError> {
    let (parts, body) = req.into_parts();
    let mut res = Response::new(body);
    *res.headers_mut() = parts.headers;
    Ok(res)
}
