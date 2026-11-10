# Personal VM synthetic authentication fixture v1

The `Synthetic personal login, MFA, and browser restart` job runs a real
Playwright Chromium process inside the repository's pinned browser QA image.
The fixture IdP binds only to loopback, the container runs with
`--network none`, and all credentials and one-time codes are synthetic values
under the reserved `.test` domain. The test rejects an invalid MFA code, accepts
the fixed fixture code, closes Chromium, reopens the same browser profile, and
checks the authenticated account page without reading or exporting cookies.

Rust tests separately verify that a `PersonalPersistent` guest authority
replacement fences the previous agent during human takeover and fences the
human authority on handback. Session journal recovery preserves only control
and lifecycle metadata; it cannot contain form values, passwords, MFA codes,
cookies, or browser observations. A guest helper contract test keeps Chromium
profile and cache paths under `/home/koolade`, which is part of the encrypted
personal disk rather than volatile `/run` storage.

These deterministic checks do not boot QEMU, exercise the guest display stream,
or validate an external identity provider. The browser restart test reuses the
profile directory across Chromium process restart; real guest reboot and GUI
takeover acceptance remains part of the real-KVM matrix in #392. R3 remains
disabled until its independent release gate passes. No real credentials or
accounts are used.
