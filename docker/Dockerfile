FROM rust

COPY . /project
WORKDIR /project

RUN apt-get update
RUN apt-get install -y gcc-aarch64-linux-gnu

RUN rustup toolchain install stable-aarch64-unknown-linux-gnu
RUN rustup target add aarch64-unknown-linux-gnu

RUN cargo update
RUN cargo build --release --target=aarch64-unknown-linux-gnu


FROM --platform=linux/arm64 rust
WORKDIR /project
COPY --from=0 /project .

RUN cd target && mv aarch64-unknown-linux-gnu/release .

CMD cargo run --release