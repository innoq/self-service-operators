FROM rust:latest as dependency-builder
RUN apt-get update && apt-get upgrade -y && apt-get install -y musl-tools upx-ucl
WORKDIR /rust-project

RUN rustup target add x86_64-unknown-linux-musl
RUN cargo install cargo-build-dependencies
RUN cargo install --force cargo-strip
COPY Cargo.toml Cargo.lock ./
RUN cargo build-dependencies --release --target=x86_64-unknown-linux-musl

FROM dependency-builder as builder
COPY . .
RUN cargo build              --release --target=x86_64-unknown-linux-musl
RUN ls -lah target/x86_64-unknown-linux-musl/release/self-service-project-operator

RUN cargo strip                        --target=x86_64-unknown-linux-musl
RUN ls -lah target/x86_64-unknown-linux-musl/release/self-service-project-operator

RUN strip target/x86_64-unknown-linux-musl/release/self-service-project-operator
RUN ls -lah target/x86_64-unknown-linux-musl/release/self-service-project-operator

RUN upx -9 target/x86_64-unknown-linux-musl/release/self-service-project-operator
RUN ls -lah target/x86_64-unknown-linux-musl/release/self-service-project-operator

FROM scratch as self-service-project-operator
COPY --from=builder rust-project/target/x86_64-unknown-linux-musl/release/self-service-project-operator /
ENTRYPOINT ["/self-service-project-operator"]

