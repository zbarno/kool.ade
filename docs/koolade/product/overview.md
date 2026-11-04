# Kool.ad/e product

Kool.ad/e is a native desktop application for clarifying software ideas,
maintaining published product knowledge, and coordinating implementation.

The application has two data authorities:

- The project's ordinary code Git repository contains source, published
  product and feature specifications, architecture decisions, and documentation.
- Installation-local application storage contains task intents and trees,
  conversations, decisions, approvals, execution state, and QA evidence.

Collaborators exchange published product knowledge through normal Git review.
Task histories and approvals remain private to the installation that created
them. There is no separate planning repository or shared operational task store.

Planning recommendations remain advisory. Starting work, approving a plan,
publishing specifications, and executing workers require the appropriate human
authorization and existing review gates.
