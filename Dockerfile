FROM rust:latest as dependency-builder
RUN apt-get update && apt-get upgrade -y && apt-get install -y musl-tools
WORKDIR /rust-project

RUN rustup target add x86_64-unknown-linux-musl
RUN cargo install cargo-build-dependencies
COPY Cargo.toml Cargo.lock ./
RUN cargo build-dependencies --release --target=x86_64-unknown-linux-musl

FROM dependency-builder as builder
COPY . .
RUN cargo build              --release --target=x86_64-unknown-linux-musl

FROM scratch as self-service-project-operator
COPY --from=builder rust-project/target/x86_64-unknown-linux-musl/release/self-service-project-operator /
ENTRYPOINT ["/self-service-project-operator"]

