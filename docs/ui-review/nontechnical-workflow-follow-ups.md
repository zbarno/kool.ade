# Nontechnical workflow and recovery follow-ups

This note records usability issues found while following a project from setup through plan review and task creation. The app is for people who should not need to understand provider names, repository internals, or lifecycle jargon to keep their work moving.

## Addressed in the current lifecycle fix

- A review that could not finish now has different wording from a review that found plan issues.
- The review card offers a clearly named retry only when the saved state allows it. The retry is bound to the same approved plan version, and the card explains that retrying neither edits the plan nor creates tasks.
- The task-generation card explains that review is the blocker and links back to the review card instead of asking for unrelated repository approval.
- Manager review uses the app's scoped evidence tool and can continue without Docker. Only roles that run repository tools require Docker, so a missing Docker target no longer looks like a broken Manager sign-in.

## Follow-ups for future consideration

### Resume the saved project after restart

Opening the app after a restart may return someone to setup instead of their saved project and task. Offer a prominent **Resume saved work** action, or restore the last project and selected task automatically. Keep setup status secondary when the user already has work to continue.

### Confirm folder connection immediately

After a person chooses a project folder, show success or a useful failure beside that action. Keep the chosen location visible on failure and offer a direct retry. Avoid sending the person to a different welcome screen to discover what happened.

### Explain saved-project compatibility problems accurately

When a project cannot be read because it was saved by a different app version, describe that as a compatibility issue and give a safe next step, such as updating the app or reopening with a compatible version. Keep parser and schema details optional; do not suggest changing folder permissions when access is not the problem.

### Separate project progress from computer setup

Make it clear which work can continue while computer checks are pending, which actions are blocked, and what resolves each blocker. Put detailed checks behind an optional section so they do not compete with the user's project goal.

### Make provider recovery understandable and current

Describe the user-visible result first, such as “We could not verify your sign-in; access is still unverified.” Show when the last attempt ran and whether the message still applies. Keep provider and filesystem terms in optional technical details, and provide a clear next step when one is available.

### Keep next steps tied to the actual blocker

Cards should name the current blocker before offering unrelated actions. If review is incomplete, do not ask the person to change the plan or approve repository actions as the next step. If a safe retry is unavailable, say so plainly and explain what the person can do next.

### Show which publication a decision authorizes

Plan approval and implementation publication happen at different points. Before asking for approval, show which feature document may be proposed and when that can happen. Keep the later implementation review and publication hold visible as a separate step so a person can tell what will become public and what is still waiting for their decision.
