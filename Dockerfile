# release stripped              : 14M self-service-project-operator
#
# -1 (0.5 seconds):
# release stripped & compressed : 6.2M self-service-project-operator
#
# -9 (12 seconds):
# release stripped & compressed : 5.2M self-service-project-operator
#
# --brute (15 minutes!):
# release stripped & compressed : 3.7M self-service-project-operator
#
ARG COMPRESSION_FACTOR="-1"

ARG RUST_BUILDER_IMAGE=ekidd/rust-musl-builder:latest

# sensible choices are scratch, busybox, alpine
ARG RUNTIME_IMAGE=alpine

ARG TARGET=x86_64-unknown-linux-musl
ARG BIN=self-service-project-operator
ARG ARTIFACT=target/${TARGET}/release/${BIN}

################################################### planner stage (collect dependencies)
FROM ${RUST_BUILDER_IMAGE} as planner
WORKDIR /app
RUN cargo install cargo-chef
COPY . .
RUN cargo chef prepare --recipe-path recipe.json

################################################### cacher stage (build dependencies)
FROM ${RUST_BUILDER_IMAGE} as cacher
WORKDIR /app
ARG TARGET
RUN cargo install cargo-chef
COPY --from=planner /app/recipe.json recipe.json
RUN cargo chef cook --target="${TARGET}" --release --recipe-path recipe.json

################################################### builder stage (build binary)
FROM ${RUST_BUILDER_IMAGE} as builder
WORKDIR /app
ARG TARGET
ARG ARTIFACT
COPY --from=cacher /home/rust/.cargo /home/rust/.cargo
COPY --from=cacher /app/target target
COPY . .
RUN cargo build --target="${TARGET}" --release --bin self-service-project-operator
RUN echo "release                       : $(cd $(dirname ${ARTIFACT}); ls -lah $(basename ${ARTIFACT})|tr -s " "|cut -f5,9 -d" ")" >  /tmp/info
RUN strip ${ARTIFACT}
RUN echo "release stripped              : $(cd $(dirname ${ARTIFACT}); ls -lah $(basename ${ARTIFACT})|tr -s " "|cut -f5,9 -d" ")" >> /tmp/info

################################################### compressor stage (compress binary)
FROM alpine as compressor
ARG ARTIFACT
ARG BIN
ARG COMPRESSION_FACTOR
WORKDIR /app
RUN apk add --no-cache upx
COPY --from=builder /app/${ARTIFACT} /app/${ARTIFACT}
RUN cd /app && ln -sf ${ARTIFACT} app
COPY --from=builder --chmod=0777 /tmp/info /tmp
RUN upx ${COMPRESSION_FACTOR} ${ARTIFACT}
RUN echo -e "$(cat /tmp/info)\nrelease stripped & compressed : $(cd $(dirname ${ARTIFACT}); ls -lah $(basename ${ARTIFACT})|tr -s " "|cut -f5,9 -d" ")"

################################################### final stage (copy binary in run time image)
FROM ${RUNTIME_IMAGE} as runtime
ARG ARTIFACT
ARG BIN

COPY --from=compressor /app/${ARTIFACT} /project-operator
ENTRYPOINT ["/project-operator"]
