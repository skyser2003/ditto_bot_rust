FROM rust

COPY . /project
WORKDIR /project

RUN cargo update
RUN cargo build

CMD cargo run