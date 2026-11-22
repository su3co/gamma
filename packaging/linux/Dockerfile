# Build environment for gamma's Linux packages (AppImage, .deb, .rpm), for
# x86_64 and aarch64. Ubuntu 22.04 keeps the glibc baseline low; the CUDA
# development image provides the headers lightsim's CUDA bindings are
# generated from (the program loads the NVIDIA driver at run time, so the
# packages run on machines without a GPU too).
ARG BASE=nvidia/cuda:13.0.1-devel-ubuntu22.04
FROM ${BASE}
ENV DEBIAN_FRONTEND=noninteractive
RUN apt-get update && apt-get install -y --no-install-recommends \
      build-essential curl wget file git pkg-config ca-certificates xdg-utils \
      libssl-dev libwebkit2gtk-4.1-dev libgtk-3-dev libayatana-appindicator3-dev librsvg2-dev libxdo-dev \
      patchelf rpm clang libclang-dev python3 xvfb xauth \
    && rm -rf /var/lib/apt/lists/*
RUN curl -fsSL https://deb.nodesource.com/setup_22.x | bash - && apt-get install -y --no-install-recommends nodejs && rm -rf /var/lib/apt/lists/*
ENV RUSTUP_HOME=/opt/rustup CARGO_HOME=/opt/cargo PATH=/opt/cargo/bin:$PATH APPIMAGE_EXTRACT_AND_RUN=1
RUN curl -fsSL https://sh.rustup.rs | sh -s -- -y --profile minimal --default-toolchain 1.99.1 && chmod -R a+rwX /opt/rustup /opt/cargo
WORKDIR /src
