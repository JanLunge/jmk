# Build from the server directory:  docker build -t jmk-registry server/
FROM rust:1-slim-bookworm AS build
WORKDIR /src
COPY Cargo.toml Cargo.lock ./
COPY src ./src
RUN cargo build --release --locked \
    # An empty directory for the runtime image to copy, since it has no shell to make one.
    && mkdir /data

# glibc and nothing else: no shell, no package manager, runs as a non-root user.
FROM gcr.io/distroless/cc-debian12:nonroot
COPY --from=build /src/target/release/jmk-registry /usr/local/bin/jmk-registry
COPY --from=build --chown=nonroot:nonroot /data /data
ENV JMK_BIND=0.0.0.0:8787 \
    JMK_DB=/data/jmk-registry.db
VOLUME /data
EXPOSE 8787
ENTRYPOINT ["/usr/local/bin/jmk-registry"]
