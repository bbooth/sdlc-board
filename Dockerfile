# The board server and the web build it serves at /, in one image.
FROM rust:1.97-bookworm AS build
WORKDIR /src
RUN rustup target add wasm32-unknown-unknown
# wasm-bindgen-cli must match the wasm-bindgen in Cargo.lock.
RUN cargo install --locked wasm-bindgen-cli --version 0.2.130
COPY . .
RUN cargo build --locked --release -p board-server \
    && CARGO_TARGET_DIR=target crates/app/web/build.sh /out/web \
    && cp target/release/board-server /out/board-server

# cc: the server links glibc. :nonroot runs as uid 65532 and writes nothing.
FROM gcr.io/distroless/cc-debian12:nonroot
COPY --from=build /out/board-server /board-server
COPY --from=build /out/web /web
ENV BOARD_WEB_DIR=/web BOARD_CONFIG=/etc/board/board.toml
EXPOSE 8080 9091
ENTRYPOINT ["/board-server"]
