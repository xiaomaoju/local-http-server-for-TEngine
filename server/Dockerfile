# Stage 1: Build web-admin
FROM node:20-alpine AS web-builder
WORKDIR /app/web-admin
COPY web-admin/package.json web-admin/package-lock.json* ./
RUN npm ci
COPY web-admin/ ./
RUN npm run build

# Stage 2: Build server
FROM rust:1.90-bookworm AS server-builder
WORKDIR /app
COPY server/Cargo.toml server/Cargo.lock* ./server/
COPY server/src/ ./server/src/
COPY --from=web-builder /app/web-admin/dist ./web-admin/dist
WORKDIR /app/server
RUN cargo build --release

# Stage 3: Runtime
FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y ca-certificates && rm -rf /var/lib/apt/lists/*
COPY --from=server-builder /app/server/target/release/tengine-server /usr/local/bin/tengine-server
RUN mkdir -p /data
EXPOSE 8080
CMD ["tengine-server"]
