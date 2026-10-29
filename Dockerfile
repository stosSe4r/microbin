FROM rust:1-bookworm AS build

WORKDIR /app

COPY . .

RUN CARGO_NET_GIT_FETCH_WITH_CLI=true cargo build --locked --release && \
    mkdir -p /app/pasta_data

FROM debian:bookworm-slim

WORKDIR /app

# copy time zone info from build stage
COPY --from=build /usr/share/zoneinfo /usr/share/zoneinfo

# copy CA certificates from build stage
COPY --from=build /etc/ssl/certs/ca-certificates.crt /etc/ssl/certs/ca-certificates.crt

# copy built executable
COPY --from=build /app/target/release/microbin /usr/bin/microbin

# copy data directory skeleton with nonroot ownership
COPY --from=build --chown=65532:65532 /app/pasta_data /app/pasta_data

ENV MICROBIN_DATA_DIR=/app/pasta_data

USER 65532:65532

VOLUME ["/app/pasta_data"]

# Expose webport used for the webserver to the docker runtime
EXPOSE 8080

ENTRYPOINT ["/usr/bin/microbin"]
