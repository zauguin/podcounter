FROM clux/muslrust:stable AS builder
COPY . .
RUN cargo build --release --features server

FROM scratch
COPY --from=builder /volume/target/x86_64-unknown-linux-musl/release/podcounter /podcounter

EXPOSE 3000
CMD ["/podcounter"]

# FROM rust:latest AS base
# RUN apt-get update && apt-get install -y --no-install-recommends libclang-dev && rm -rf /var/lib/apt/lists/*
# RUN cargo install sccache --version ^0.7
# RUN cargo install cargo-chef --version ^0.1
# ENV RUSTC_WRAPPER=sccache SCCACHE_DIR=/sccache
#  
# FROM base AS planner
# WORKDIR /app
# COPY . .
# RUN --mount=type=cache,target=$SCCACHE_DIR,sharing=locked \
#     cargo chef prepare --recipe-path recipe.json
#  
# FROM base as builder
# WORKDIR /app
# COPY --from=planner /app/recipe.json recipe.json
# RUN --mount=type=cache,target=$SCCACHE_DIR,sharing=locked \
#     cargo chef cook --release --recipe-path recipe.json
# COPY . .
# RUN --mount=type=cache,target=$SCCACHE_DIR,sharing=locked \
#     cargo build --release
# 
# FROM gcr.io/distroless/cc-debian12:nonroot as runner
# COPY --from=builder /app/target/release/podcounter /app/
# WORKDIR /app
# 
# EXPOSE 3000
# CMD ["/app/podcounter"]
