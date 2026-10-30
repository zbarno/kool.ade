# Welcome onboarding usability session

Status: test script prepared; implementation and validation are not complete.
Run against the final candidate build, not an earlier screenshot or fixture.
Record the candidate commit, package version, Linux distribution, desktop,
and whether the participant has prior terminal, Git, Docker or coding-tool
experience. Use synthetic project names and ideas. Never publish personal
paths, account details, credentials or transcripts containing private data.

## Preparation

Use a disposable user profile with no saved application setup or projects.
Have a supported provider account available for its own secure login flow.
Do not give the participant terminal commands or explain the UI in advance.
The observer records confusion before intervening. Stop if an operation's
preview is unclear or differs from the requested action.

## Participant tasks

1. Launch Kool.ad/e and explain what you think it will help you do.
2. Choose an assistant, complete provider-owned login as needed, and explain
   which actions the assistant can perform before the computer is ready.
3. Review a real missing prerequisite. Explain the proposed repair, approve
   only the intended change, and follow the application's guidance. Decline
   another repair and check that planning remains available when safe.
4. Close and reopen during an unfinished setup step. Explain what resumed
   and confirm that no earlier permission was silently reused.
5. Create “Garden notes” with the idea “Help me track planting dates”. Review
   where code and planning history will live before confirming creation.
   Check that the idea appears in the normal planning conversation.
6. Open an existing non-Git folder containing synthetic files. Review and
   approve history initialization; verify those files are unchanged.
7. Open an existing Git project with an uncommitted change. Confirm that its
   branch, staged files and working changes are preserved.
8. Open several workspaces, restart, and find the most recently opened one.
   Move one disposable workspace folder and try reconnecting it without
   creating a replacement workspace or losing history.
9. Resize to a narrow window and navigate using Tab, Shift+Tab, Enter and
   Escape. Ask for help during setup and project selection.

## Observer record

For each task, record outcome (completed / assisted / blocked), observed
confusion, exact UI action needed, and any unexpected changes. Record wide
and narrow screenshots using synthetic data. Avoid recording provider login
screens or private diagnostics. Link each actionable finding to its fix and
repeat the affected task on the corrected candidate.

A developer walkthrough verifies real behavior but does not automatically
satisfy the issue's observed novice requirement. Record participant experience
honestly. Automated fixtures do not substitute for this session. Platform
claims remain limited to the hosts actually tested.
