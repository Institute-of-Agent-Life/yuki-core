FROM rust:1.96.0-slim-bookworm@sha256:4732ca96fd086cb9be682050c3f0176288eebaac2b80aa2bcefccfaf198e1950 AS build
RUN apt-get update && apt-get install -y --no-install-recommends g++ && rm -rf /var/lib/apt/lists/*
WORKDIR /src
COPY Cargo.toml Cargo.lock rust-toolchain.toml build.rs ./
COPY sdk/rust/ sdk/rust/
COPY protocol/plugin/gateway/ protocol/plugin/gateway/
COPY src/ src/
COPY build/ build/
COPY native/opencc/ native/opencc/
COPY migrations/ migrations/
COPY character/SOUL.md character/SOUL.md
COPY config/qq_face_map.json config/qq_face_map.json
RUN cargo fetch --locked
RUN cargo build --release --locked --offline

FROM debian:bookworm-slim@sha256:99c0b25c46d0214d5a2091092e7908907dcaecd6a962cde0ac700acee222b6f3
COPY --from=build /src/target/release/yuki-core /usr/local/bin/yuki-core
COPY LICENSE NOTICE /usr/share/licenses/yuki-core/
COPY native/opencc/LICENSES.txt /usr/share/licenses/yuki-core/OpenCC-LICENSES.txt
USER 65532:65532
ENTRYPOINT ["yuki-core"]
