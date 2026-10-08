# Security

Kool.ad/e is experimental and has not been fully security-vetted. It runs
external coding agents that can inspect and change repositories. Autonomous
execution carries risk; use disposable test repositories, keep independent
backups, review generated changes, and leave automatic publication disabled
unless you knowingly accept the risk.

The Bubblewrap sandbox, isolated task repositories, approval steps, and verification
checks reduce risk. They are not a formal security boundary or a guarantee that
the host, repository, or credentials cannot be exposed or changed unexpectedly.

Do not include live credentials, access tokens, private keys, or passwords in a
security report. Reproduce issues on a disposable or test repository and redact
secrets, personal data, and private machine details from the report.
