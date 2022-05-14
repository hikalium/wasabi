FROM ubuntu:20.04
# Use bash
SHELL ["/bin/bash", "-c"]
# To avoid being asked about tzdata
ARG DEBIAN_FRONTEND=noninteractive
# Install dependencies
# gcc-multilib for bits/libc-header-start.h
RUN apt-get update \
    && apt-get install -y --no-install-recommends \
	build-essential \
	ca-certificates \
	clang \
	clang-8 \
	clang-format \
	cmake \
	curl \
	gcc \
	git \
	libc++-8-dev \
	libc++abi-8-dev \
	lld \
	lld-8 \
	llvm-8-tools \
	make \
	netcat \
	qemu-system-x86 \
	qemu-utils \
	rsync \
	wget \
	gcc-multilib \
	python3-pip \
	telnet \
	&& apt-get -q -y autoremove \
	&& apt-get -q -y clean \
	&& rm -rf /var/lib/apt/lists/*
# Install rust toolchain
RUN curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | \
	sh -s -- -y --default-toolchain=nightly-x86_64-unknown-linux-gnu
ENV PATH $PATH:/root/.cargo/bin
ARG RUST_TOOLCHAIN=nightly-2022-01-01-x86_64-unknown-linux-gnu
RUN rustup toolchain install ${RUST_TOOLCHAIN}
RUN rustup component add rust-src --toolchain nightly-2022-01-01-x86_64-unknown-linux-gnu
RUN echo 'export PS1="(wasabi-builder)$ "' >> /root/.bashrc


CMD ["/bin/bash"]
