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
ARG COMPRESSION_FACTOR="-9"

ARG RUST_BUILDER_IMAGE=ekidd/rust-musl-builder:latest

# sensible choices are scratch, busybox (if you need a shell), alpine (if you need a shell + package manager)
ARG RUNTIME_IMAGE=scratch

#ARG TARGET=x86_64-unknown-linux-musl
ARG TARGET=x86_64-unknown-linux-gnu

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
RUN strip ${ARTIFACT}

# get all dynamic dependencies
RUN bash -c "echo ${ARTIFACT} > /tmp/deps;\
    while ! diff /tmp/deps /tmp/new_deps &>/dev/null; do \
      mv -f /tmp/new_deps /tmp/deps 2>/dev/null;\
      while read file; do \
        echo \$file >> /tmp/new_deps_tmp;\
        ldd \$file |grep '=>'   |grep '/'|tr -s ' \t' '\t'|cut -f4|sort|uniq >> /tmp/new_deps_tmp;\
        ldd \$file |grep -v '=>'|grep '/'|tr -s ' \t' '\t'|cut -f2|sort|uniq >> /tmp/new_deps_tmp;\
      done < /tmp/deps;\
      cat /tmp/new_deps_tmp|sort|uniq|grep -v '^\$' > /tmp/new_deps;\
    done;\
    while read file; do\
      (set -x; install -Ds \$file /tmp/buildroot/\${file});\
    done < <(cat /tmp/deps|grep -v '${ARTIFACT}');\
    touch /tmp/buildroot"

################################################### compressor stage (compress binary)
FROM alpine as compressor
ARG ARTIFACT
ARG BIN
ARG COMPRESSION_FACTOR
WORKDIR /app
RUN apk add --no-cache upx
COPY --from=builder /app/${ARTIFACT} /app/${ARTIFACT}
RUN cd /app && ln -sf ${ARTIFACT} app
RUN upx ${COMPRESSION_FACTOR} ${ARTIFACT}

################################################### final stage (copy binary in run time image)
FROM ${RUNTIME_IMAGE} as runtime
ARG ARTIFACT
ARG BIN

COPY --from=builder /tmp/buildroot/ /
COPY --from=compressor /app/${ARTIFACT} /project-operator
COPY --from=compressor /etc/ssl /etc/ssl
ENTRYPOINT ["/project-operator"]