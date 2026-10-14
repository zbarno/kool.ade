# `scratch` makes an omitted build argument unusable instead of selecting a
# mutable or implicit base. CI always supplies a locked architecture variant.
ARG BASE_IMAGE=scratch
FROM ${BASE_IMAGE}

ARG EXECUTOR_PROFILE=general

# Build from this trusted directory only. BASE_IMAGE must be selected from
# containers/images.lock.json and include an architecture-specific sha256 digest.
LABEL org.opencontainers.image.title="Kool.ad/e ${EXECUTOR_PROFILE} executor" \
      org.opencontainers.image.source="https://github.com/zbarno/kool.ade" \
      org.opencontainers.image.version="1" \
      com.koolade.executor.profile="${EXECUTOR_PROFILE}"

# Toolchain source images can inherit package-manager clients and Git from
# buildpack-oriented bases. The executor has no network and needs none of them.
RUN case "$EXECUTOR_PROFILE" in \
      rust|node|python|go) \
        for package in curl wget git; do \
          if dpkg-query -W "$package" >/dev/null 2>&1; then dpkg --remove "$package"; fi; \
        done ;; \
    esac

USER 10001:10001
WORKDIR /workspace
ENTRYPOINT ["/bin/sh"]
