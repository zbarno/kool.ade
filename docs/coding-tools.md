# Coding tool discovery

Kool.ad/e asks the installed Codex app-server for its model catalog and uses the CLI's reported model choices in settings. OpenAI documents that `model/list` can return a bundled or cached catalog; it is not an account entitlement check. For current account-specific choices, OpenAI recommends querying `/v1/models` with that account's OAuth access token. Kool.ad/e does not read a Codex credential store or make that account-specific request, so a listed model can still be unavailable to the signed-in account.

Sources: [Codex app-server](https://developers.openai.com/siwc/token-sharing-open-source/codex-app-server), [Models and inference](https://developers.openai.com/siwc/token-sharing-open-source/models-and-inference).
