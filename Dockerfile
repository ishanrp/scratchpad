FROM archlinux:base-devel
RUN pacman -Syu --noconfirm rust gtk4 gtk4-layer-shell sqlite pkgconf git && pacman -Scc --noconfirm
WORKDIR /src
COPY . .
RUN cargo build --workspace --release
RUN mkdir -p /dist \
    && cp target/release/scratchpad-daemon /dist/ \
    && cp target/release/scratchpad /dist/scratchpad-cli \
    && cp target/release/scratchpad-ui /dist/
