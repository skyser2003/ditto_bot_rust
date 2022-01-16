FROM rust:1.57-alpine AS builder

WORKDIR /ws

ARG BUILD_HOST_MODE=cross
ARG BUILD_ARCH=aarch64
ARG BINARY_NAME=ditto-bot-rust

RUN apk add --no-cache \
    musl-dev \
    curl \
    ca-certificates \
    openssl-dev
RUN rustup target add ${BUILD_ARCH}-unknown-linux-musl
RUN curl https://musl.cc/${BUILD_ARCH}-linux-musl-${BUILD_HOST_MODE}.tgz -o ${BUILD_ARCH}-linux-musl-${BUILD_HOST_MODE}.tgz && \
    tar -xzf ${BUILD_ARCH}-linux-musl-${BUILD_HOST_MODE}.tgz && \
    rm ${BUILD_ARCH}-linux-musl-${BUILD_HOST_MODE}.tgz && \
    mv ${BUILD_ARCH}-linux-musl-${BUILD_HOST_MODE} /
RUN mkdir .cargo && \
  echo -e "[target.${BUILD_ARCH}-unknown-linux-musl]\n\
linker = \"${BUILD_ARCH}-linux-musl-gcc\"\n\
ar = \"${BUILD_ARCH}-linux-musl-ar\"\n\
rustflags = [\"-Ctarget-feature=+crt-static\"]" > .cargo/config

ENV PATH=${PATH}:/${BUILD_ARCH}-linux-musl-${BUILD_HOST_MODE}/bin

ADD Cargo.toml ./
ADD src/ ./src

ENV PKG_CONFIG_ALL_STATIC=1
RUN cargo build --release --target ${BUILD_ARCH}-unknown-linux-musl

FROM --platform=linux/arm64 scratch

ARG BUILD_ARCH=aarch64
ARG BINARY_NAME=ditto-bot-rust

COPY --from=builder /ws/target/${BUILD_ARCH}-unknown-linux-musl/release/${BINARY_NAME} /
COPY --from=builder /etc/ssl/certs/ca-certificates.crt /etc/ssl/certs/

ENTRYPOINT ["${BINARY_NAME}"]
