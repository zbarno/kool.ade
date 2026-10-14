# Runtime QA browser driver

`driver.mjs` is the app-owned, one-shot Playwright driver. It accepts a bounded
JSON request uploaded to a fixed tmpfs path, starts a fresh Chromium process and browser context, and
returns action observations, screenshots (PNG bytes plus SHA-256), console
errors, failed requests and the exact Playwright/browser versions. It accepts
only the typed `Browser*` actions from `QaDriverActionV1`; arbitrary JavaScript,
commands, extensions, downloads, persistent profiles and browser credentials
are not part of the protocol.

The request origin is the app-owned `http://qa-preview:8080` relay. HTTP
requests, redirects, frames, and WebSockets are restricted to that exact
origin. The `qa-service` alias in scenario actions resolves to the relay. The
relay accepts one task-service port on one internal Docker network, and the
browser reaches it through a second internal network. It publishes no host
ports and rejects requests with any other Host header. Screenshot count (64), total evidence bytes (64 MiB), per-image bytes (8 MiB), viewport,
action count, per-action timeout, and total run time are bounded by both the request
and the driver.

`Dockerfile` derives from the verified, digest-pinned Microsoft Playwright
1.57.0 Noble image. It installs the locked Playwright npm packages offline from
the checked-in tarballs whose SHA-512 values match `package-lock.json`; the
base already contains Chromium 143.0.7499.4, which the smoke workflow checks.
The derived image runs as UID/GID 10001. Only linux/amd64 is verified. The lock
records the base digest and a recipe digest, while runtime selection stays
disabled until the derived OCI image digest is published and pinned.

The synthetic web target at `tests/fixtures/runtime_qa/browser/server.py` uses
only Python's standard library. It covers form validation and persistence,
modal behavior, responsive layout, plus seeded clipping, wrong-action, and
JavaScript-error defects. It is a project fixture; it does not run in the
browser-driver image.

## Current availability

The derived image recipe and strict local smoke are present, but the app does
not yet have a pinned released image digest or route Runtime QA through the
two-network Docker session. Browser QA remains unavailable until that app-owned
lifecycle is wired. The driver source and fixture alone are not a QA pass or a
release claim.
