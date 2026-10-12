[![Build, deploy, test](https://github.com/skyser2003/ditto_bot_rust/workflows/Rust/badge.svg)](https://github.com/skyser2003/ditto_bot_rust/actions?query=workflow%3ARust)

# Usage

```bash
SLACK_BOT_TOKEN=$() SLACK_SIGNING_SECRET=$() cargo run
```

# Test

```bash
cargo test -- --no-capture
```

`test::MockBot`을 활용해서 메시지 테스트를 하면 slack에 post하는 메시지 데이터를 볼 수 있다.
https://api.slack.com/docs/messages/builder 에서 해당 메시지가 올바른지 테스트 해 볼 수 있다.

## Release deployment

Push to `release` to run format checks, release tests, and Clippy, then build and publish the ARM64 image used by the rootless K3s server. GitHub Actions deploys through the `ditto-bot` Argo CD application and waits for the new deployment generation to become ready.

Repository Actions Secrets:

- `REGISTRY_URL`, `REGISTRY_USERNAME`, and `REGISTRY_PASSWORD`: container registry access.
- `ARGOCD_URL`: the Argo CD base URL, including `https://`.
- `ARGOCD_AUTH_TOKEN`: a project-role token permitted to read and restart only `skyser/ditto-bot`.

The deployment uses Argo CD's Deployment restart action so the `latest` image is pulled immediately. Portainer is no longer part of the release process. Token revocation and renewal are managed through the `ditto-bot-actions` role in the `skyser` Argo CD project.
