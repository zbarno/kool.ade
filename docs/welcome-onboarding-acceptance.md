# Welcome workspace setup acceptance

Welcome connects to one ordinary code Git repository. It creates the local
Kool.ad/e project data automatically and does not provision or pair a planning
repository. A blank code repository may be used without a language or framework
scaffold.

## Automated coverage

Automated setup coverage must prove that a clean installation can create and
open a workspace from a normal code repository, that repository documents are
proposed without overwriting existing human content, and that the app neither
creates a dedicated planning repository nor derives a `.packet` remote. The app must create private local
workspace metadata without writing task, approval, conversation, or QA records
to the source repository. Setup errors must not create partial external GitHub
repositories or weaken existing Git authorization.

## Manual acceptance deferred

The human-operated walkthrough remains unperformed. The exact required
scenarios are listed in [welcome-onboarding-usability.md](welcome-onboarding-usability.md):
fresh setup, existing human docs, private-record restart recovery, independent
local installations, pinned-spec behavior across branch switches, reviewed
publication, and keyboard/narrow-window usability. Record the actual
candidate build and results when run. Do not report automated fixtures as
manual acceptance.

This acceptance applies to the greenfield architecture.
