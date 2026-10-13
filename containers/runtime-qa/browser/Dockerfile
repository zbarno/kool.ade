ARG BASE_IMAGE=mcr.microsoft.com/playwright@sha256:8fb7af3bb488c51364d6554876a8eddf377736608327dbdf4177b4901faf7bc9
FROM ${BASE_IMAGE}

ARG SOURCE_DATE_EPOCH=0
USER root
WORKDIR /opt/koolade-runtime-qa
COPY package.json package-lock.json ./
COPY vendor/ ./vendor/
RUN node -e 'const fs=require("node:fs"),crypto=require("node:crypto"),lock=JSON.parse(fs.readFileSync("package-lock.json")); for (const name of ["playwright","playwright-core"]) { const file=`vendor/${name}-1.57.0.tgz`; const actual=`sha512-${crypto.createHash("sha512").update(fs.readFileSync(file)).digest("base64")}`; if (lock.packages[`node_modules/${name}`]?.integrity !== actual) process.exit(1); }' \
    && mkdir -p /tmp/koolade-qa-packages /opt/koolade-runtime-qa/node_modules \
    && tar -xzf vendor/playwright-1.57.0.tgz -C /tmp/koolade-qa-packages \
    && mv /tmp/koolade-qa-packages/package /opt/koolade-runtime-qa/node_modules/playwright \
    && tar -xzf vendor/playwright-core-1.57.0.tgz -C /tmp/koolade-qa-packages \
    && mv /tmp/koolade-qa-packages/package /opt/koolade-runtime-qa/node_modules/playwright-core \
    && rm -rf vendor /tmp/koolade-qa-packages
COPY --chown=10001:10001 driver.mjs ./driver.mjs
COPY --chown=10001:10001 relay.mjs ./relay.mjs
RUN chown -R 10001:10001 /opt/koolade-runtime-qa \
    && find /opt/koolade-runtime-qa -exec touch -h -d "@${SOURCE_DATE_EPOCH}" {} +
USER 10001:10001
ENV HOME=/tmp
ENTRYPOINT ["node", "/opt/koolade-runtime-qa/driver.mjs"]
