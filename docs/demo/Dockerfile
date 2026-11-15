# SPDX-License-Identifier: MIT OR Apache-2.0
# The image that records docs/demo/demo.gif: kiwa built from this checkout, on top of the VHS
# image (which brings ttyd and ffmpeg). Built and run by `cargo xtask demo`; the build context
# is the repository's root (see .dockerignore). Images are pinned by digest; Dependabot updates them.

FROM rust:1-bookworm@sha256:ade39a710ef85f3506cffcfec3e8a24ba090badef1dff2fb798f71d878833fea AS build
WORKDIR /src
COPY . .
RUN cargo build --release --locked -p kiwa

FROM ghcr.io/charmbracelet/vhs:v0.9.0@sha256:4ed9aec4479b572958aea98f41160d0ff90db8a7a4fe34517186237b2e880f89
# Node and npm run the demo's `hello greet` task. The image follows Debian `stable`, whose codename
# changes between releases (bookworm → trixie): accept it rather than fail.
RUN apt-get update --allow-releaseinfo-change \
    && apt-get install -y --no-install-recommends nodejs npm \
    && rm -rf /var/lib/apt/lists/*
COPY --from=build /src/target/release/kiwa /src/target/release/kw /usr/local/bin/
