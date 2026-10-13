[![Build, deploy, test](https://github.com/skyser2003/ditto_bot_rust/workflows/Rust/badge.svg)](https://github.com/skyser2003/ditto_bot_rust/actions?query=workflow%3ARust)

# Usage

```bash
SLACK_BOT_TOKEN=$() SLACK_SIGNING_SECRET=$() cargo run
```

## ChatGPT image input

Attach one or more images to a Slack message and mention the bot:

```text
@Ditto gpt Describe this image.
```

You can also send `@Ditto gpt` or just `@Ditto` with an image attached. Images
from earlier messages in the same thread are included in follow-up requests.
Supported formats are PNG, JPEG, WebP, and non-animated GIF, up to 20 MiB per
image. Other file attachments are ignored.

The Slack bot token needs the `files:read` OAuth scope. Add the scope and
reinstall the Slack app in your workspace if necessary. Set `OPENAI_MODEL` to a
model that supports image input; the default is `gpt-4o-mini`.

The bot downloads private Slack images using its Slack token and sends their
contents to the OpenAI Responses API as base64 data URLs. This works with both
streaming and non-streaming responses (`USE_GPT_STREAM`). If an image cannot be
read or is too large, the bot replies with an error before calling OpenAI.

## ChatGPT reply formatting

ChatGPT replies are converted from Markdown to Slack's native rich text for both
completed responses and streaming updates. Bold, italics, strikethrough, links,
lists, quotes, and inline or fenced code retain their formatting. Bare URLs and
email addresses remain clickable. Headings are
shown in bold, and tables are displayed as preformatted text.

The original Markdown is retained in the message's text fallback so follow-up
requests keep the assistant's response in their conversation history.

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
