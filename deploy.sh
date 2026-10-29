#!/bin/sh
set -eu
cd "$(dirname "$0")"
test -f microbin.env || cp microbin.env.example microbin.env
mkdir -p data
# The runtime image runs as uid 65532.
chown -R 65532:65532 data
docker build --tag microbin-local:latest .
if docker container inspect microbin >/dev/null 2>&1; then
    docker rm -f microbin >/dev/null
fi
exec docker run -d --name microbin --restart unless-stopped \
    --publish 31245:8080 \
    --env-file "$(pwd)/microbin.env" \
    --mount "type=bind,src=$(pwd)/data,dst=/app/pasta_data" \
    --log-opt max-size=10m --log-opt max-file=3 \
    microbin-local:latest
