# Welcome workspace setup walkthrough

Status: ready for the operator to run against the final candidate build. Record
the candidate commit, package version, Pop!_OS version, COSMIC version and
outcomes. The operator reports terminal comfort. The separate novice usability
session was waived; this walkthrough must not be described as novice testing.

Use the existing public pair `https://github.com/zbarno/kool.ade` and
`https://github.com/zbarno/kool.ade.packet`. Do not include private home paths,
credentials, account details or unredacted diagnostics in screenshots or
public notes.

## Walkthrough

1. Launch Kool.ad/e and identify the action for cloning a code repository with
   its planning repository.
2. Paste the code repository URL. Leave the optional display name blank, then
   continue. Confirm that review names both repositories, shows the managed
   local code destination and explains the separate planning checkout.
3. Confirm the operation. Check that Kool.ad/e clones the code repository and
   the same-owner `kool.ade.packet` planning repository, registers the code
   repository and opens the board.
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
7. If a safe test pair is available, verify that a missing planning repository
   is created with the code repository's visibility. Verify a visibility
   mismatch stops before either repository is changed.
8. Resize the window and use Tab, Shift+Tab, Enter and Escape. Confirm labels,
   review details, errors and primary actions remain clear and reachable.

## Observer record

For each step, record outcome (completed / assisted / blocked), any confusing
wording or action, and any unexpected files or Git changes. Link each actionable
finding to a fix and repeat the affected step on the corrected candidate.
Automated fixtures do not replace this manual check. Platform claims remain
limited to hosts actually tested.
