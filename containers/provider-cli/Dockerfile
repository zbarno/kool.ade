ARG BASE_IMAGE=scratch
FROM ${BASE_IMAGE}

ARG SOURCE_DATE_EPOCH=0
ARG CODEX_VERSION
ARG CLAUDE_VERSION
LABEL org.opencontainers.image.title="Kool.ad/e provider CLI runtime" \
      org.opencontainers.image.source="https://github.com/zbarno/kool.ade" \
      org.opencontainers.image.version="codex-${CODEX_VERSION}-claude-${CLAUDE_VERSION}" \
      com.koolade.executor.profile="provider_cli" \
      com.koolade.provider.codex.version="${CODEX_VERSION}" \
      com.koolade.provider.claude.version="${CLAUDE_VERSION}"

USER root
COPY --chown=10001:10001 binaries/codex /usr/local/bin/codex
COPY --chown=10001:10001 binaries/claude /usr/local/bin/claude
RUN chmod 0555 /usr/local/bin/codex /usr/local/bin/claude \
    && find /usr/local/bin/codex /usr/local/bin/claude -exec touch -h -d "@${SOURCE_DATE_EPOCH}" {} +

USER 10001:10001
WORKDIR /workspace
ENTRYPOINT ["/bin/sh"]
