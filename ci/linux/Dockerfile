# Linux (Debian 13, glibc) with the toolchain the repository pins and a
# system Python, for running ci/run-all.sh. Pinned by digest: the tag alone
# can move.
FROM rust:1.100.0-trixie@sha256:e19840879045c951c905d0eaa9887f3439c62000e429963371b376bac8d38c31
RUN apt-get update \
 && apt-get install -y --no-install-recommends python3 python3-venv python3-dev git ca-certificates \
 && rm -rf /var/lib/apt/lists/* \
 && rustup component add rustfmt clippy
WORKDIR /work
COPY in-container.sh /usr/local/bin/in-container.sh
ENTRYPOINT ["/usr/local/bin/in-container.sh"]
