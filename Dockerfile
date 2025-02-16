FROM rust:1.75 as builder

WORKDIR /usr/src/s3-cache
# Pre-build deps based on Cargo.toml, so we can make small changes during development
COPY rust-toolchain.toml Cargo.lock Cargo.toml .
RUN mkdir src \
    && echo "// empty" > src/lib.rs \
    && cargo build --release --locked \
    && rm -rf src/ Cargo.lock Cargo.toml
COPY . .

# ensure lib.rs is recompiled and do the real build
RUN touch src/lib.rs && cargo install --locked --path .

# Publish the result
FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y libssl3 ca-certificates && rm -rf /var/lib/apt/lists/*
COPY --from=builder /usr/local/cargo/bin/s3-cache /usr/local/bin/s3-cache
WORKDIR /data
CMD /usr/local/bin/s3-cache
RUN /usr/local/bin/s3-cache --help
