# Welcome workspace setup walkthrough

Status: ready for the operator to run against the final candidate build. Record
the candidate commit, package version, Pop!_OS version, COSMIC version and
outcomes. The operator reports terminal comfort. The separate novice usability
session was waived; this walkthrough must not be described as novice testing.

Use the public code repository `https://github.com/zbarno/kool.ade`. Before
starting, check whether its derived planning repository,
`https://github.com/zbarno/kool.ade.packet`, exists. It was missing at the last
check, so this walkthrough is intended to exercise the create path. If it is
still missing, approving setup will create and push a **public** planning
repository under `zbarno`. Do not proceed unless that public repository
creation is intended. If it now exists, this walkthrough exercises the clone
path instead. Use another safe public code repository with a missing `.packet`
pair under an owner authorized to create that public planning repository. Do
not include private home paths, credentials, account details or unredacted
diagnostics in screenshots or public notes.

## Walkthrough

1. Launch Kool.ad/e and identify the action for cloning a code repository with
   its planning repository.
2. Paste the code repository URL. Leave the optional display name blank, then
   continue. Confirm that review names both repositories, shows the managed
   local code destination and explains the separate planning checkout.
3. Review the operation first. When the planning repository is missing, verify
   that the review explains it will create the same-owner `.packet` repository
   with matching visibility. Approve only after checking that explanation.
   Then check that Kool.ad/e clones the code repository, creates and pushes the
   separate planning repository, registers the code repository and opens the
   board. When the planning repository already exists, check that Kool.ad/e
   clones it instead.
4. In the opened workspace, verify code files come from the code checkout and
   planning data comes from the planning checkout. Confirm no
   `.koolade-packet` project-artifact directory was created inside either
   checkout.
5. Quit and relaunch. Reopen the saved workspace and confirm it returns to the
   same project and planning history without another clone or duplicate
   workspace.
6. Return to Welcome and inspect the available setup choices. Confirm there is
   no control to select arbitrary existing code or planning folders and no
   embedded-planning migration path.
7. If a safe test pair is available, verify that a visibility mismatch stops
   before either repository is changed. Do not change visibility on a real
   repository to create this test condition.
8. Resize the window and use Tab, Shift+Tab, Enter and Escape. Confirm labels,
   review details, errors and primary actions remain clear and reachable.

## Observer record

For each step, record outcome (completed / assisted / blocked), any confusing
wording or action, and any unexpected files or Git changes. Link each actionable
finding to a fix and repeat the affected step on the corrected candidate.
Automated fixtures do not replace this manual check. Platform claims remain
limited to hosts actually tested.
