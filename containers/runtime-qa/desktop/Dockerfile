ARG BASE_IMAGE=mcr.microsoft.com/playwright@sha256:8fb7af3bb488c51364d6554876a8eddf377736608327dbdf4177b4901faf7bc9
FROM ${BASE_IMAGE}

ARG SOURCE_DATE_EPOCH=0
USER root
COPY --chown=10001:10001 koolade-qa-desktop /usr/local/bin/koolade-qa-desktop
RUN test -x /usr/bin/Xvfb \
    && (getent group 10001 >/dev/null || printf 'koolade:x:10001:\n' >> /etc/group) \
    && (getent passwd 10001 >/dev/null || printf 'koolade:x:10001:10001:Koolade QA:/tmp:/usr/sbin/nologin\n' >> /etc/passwd) \
    && chmod 0555 /usr/local/bin/koolade-qa-desktop \
    && mkdir -p /run/koolade-runtime-qa \
    && chown -R 10001:10001 /run/koolade-runtime-qa \
    && touch -h -d "@${SOURCE_DATE_EPOCH}" /usr/local/bin/koolade-qa-desktop /run/koolade-runtime-qa
USER 10001:10001
ENV HOME=/tmp DISPLAY=:99 LANG=C.UTF-8
ENTRYPOINT ["/usr/local/bin/koolade-qa-desktop"]
