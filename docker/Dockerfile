FROM rust

COPY . /project
WORKDIR /project

RUN cargo update
RUN cargo build --release

CMD cargo run --release
