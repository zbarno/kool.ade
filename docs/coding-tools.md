# Coding tool discovery

Kool.ad/e asks the installed Codex app-server for its paginated, non-hidden model catalog and shows the reported slugs and default in Coding tools settings. OpenAI documents that `model/list` can return a bundled or cached catalog; it is not an account entitlement check. For current account-specific choices, OpenAI recommends querying `/v1/models` with that account's OAuth access token. Kool.ad/e does not read a Codex credential store or make that account-specific request, so a listed model can still be unavailable to the signed-in account. Codex repository routing remains unavailable until it can run under Kool.ad/e's application-owned sandbox policy.

Sources: [Codex app-server](https://developers.openai.com/siwc/token-sharing-open-source/codex-app-server), [Models and inference](https://developers.openai.com/siwc/token-sharing-open-source/models-and-inference).
