ARG TARGETARCH

FROM rust:1.85-alpine AS builder

WORKDIR /ws

RUN apk add --no-cache \
  musl-dev \
  curl \
  ca-certificates \
  openssl-dev

ARG TARGETARCH
ARG BUILDARCH

# env substitution not working
# Use workaround
RUN if [ "${TARGETARCH}" = "arm64" ]; then \
  echo -n "aarch64" > TARGETARCH; \
  else \
  echo -n ${TARGETARCH} > TARGETARCH; \
  fi; \
  if [ "${TARGETARCH}" = "${BUILDARCH}" ]; then \
  echo -n "native" > BUILD_MODE; \
  else \
  echo -n "cross" > BUILD_MODE; \
  fi;

# install musl target
RUN rustup target add $(cat TARGETARCH)-unknown-linux-musl
# install musl compiler
RUN curl https://musl.cc/$(cat TARGETARCH)-linux-musl-$(cat BUILD_MODE).tgz -o $(cat TARGETARCH)-linux-musl-$(cat BUILD_MODE).tgz && \
  tar -xzf $(cat TARGETARCH)-linux-musl-$(cat BUILD_MODE).tgz && \
  rm $(cat TARGETARCH)-linux-musl-$(cat BUILD_MODE).tgz && \
  mv $(cat TARGETARCH)-linux-musl-$(cat BUILD_MODE) /gcc-musl
ENV PATH=${PATH}:/gcc-musl/bin

RUN mkdir .cargo && \
  echo -e "[target.$(cat TARGETARCH)-unknown-linux-musl]\n\
  linker = \"$(cat TARGETARCH)-linux-musl-gcc\"\n\
  ar = \"$(cat TARGETARCH)-linux-musl-ar\"\n\
  rustflags = [\"-Ctarget-feature=+crt-static\"]" > .cargo/config

# begin build
ADD Cargo.toml ./
ADD src/ ./src

ENV PKG_CONFIG_ALL_STATIC=1
RUN cargo build --release --target $(cat TARGETARCH)-unknown-linux-musl --features check-req && \
  mv target/$(cat TARGETARCH)-unknown-linux-musl/release/ditto-bot-rust /ditto-bot-rust

# XXX: using scratch create HOSTARCH image.
FROM --platform=linux/${TARGETARCH} alpine

COPY --from=builder /ditto-bot-rust /
COPY --from=builder /etc/ssl/certs/ca-certificates.crt /etc/ssl/certs/

ENTRYPOINT ["/ditto-bot-rust"]
