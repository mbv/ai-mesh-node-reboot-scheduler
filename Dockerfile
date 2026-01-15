# Build stage
FROM rust:1.92-slim AS builder

# Install build dependencies for static linking with musl (for Alpine)
RUN apt-get update && apt-get install -y --no-install-recommends \
    musl-tools \
    musl-dev \
    ca-certificates \
    && rm -rf /var/lib/apt/lists/*

# Install musl target for static linking
RUN rustup target add x86_64-unknown-linux-musl

WORKDIR /app

# Copy Cargo files
COPY Cargo.toml Cargo.lock* ./

# Create a dummy main.rs to build dependencies
RUN mkdir src && echo "fn main() {}" > src/main.rs

# Build dependencies (this layer will be cached) - static linking for musl
RUN RUSTFLAGS="-C target-feature=+crt-static" cargo build --release --target x86_64-unknown-linux-musl && rm src/main.rs

# Copy source code
COPY src ./src

# Build the actual binary - static linking for musl
RUN RUSTFLAGS="-C target-feature=+crt-static" cargo build --release --target x86_64-unknown-linux-musl

# Runtime stage - use alpine for minimal size with required libraries
FROM alpine:3.19

# Install ca-certificates for HTTPS and tzdata for timezone support
RUN apk --no-cache add ca-certificates tzdata

# Create non-root user
RUN addgroup -g 1000 appuser && \
    adduser -D -u 1000 -G appuser appuser

# Copy the statically linked binary
COPY --from=builder /app/target/x86_64-unknown-linux-musl/release/reboot_scheduler /usr/local/bin/reboot_scheduler

# Set timezone environment variable (can be overridden)
ENV TZ=UTC

# Run as non-root user
USER appuser

ENTRYPOINT ["/usr/local/bin/reboot_scheduler"]
