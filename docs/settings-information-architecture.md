# Settings information architecture

Settings are grouped by the question an operator is trying to answer. Device
configuration lives in Application settings; repository metadata lives in
Project / Git settings, while seated identity and stakeholder information live
in People & stakeholders. Coding tool availability and work
routing have separate sections because availability answers “what can run?”
while routing answers “what should run this kind of work?”.

| User concept | Current and planned options | Home |
| --- | --- | --- |
| General | Appearance, reduced motion, automation policy, persona | Application settings |
| Coding tools | Detected CLI tools, readiness, versions, default tool, rediscovery; #68 manual executable paths | Coding tools |
| Models & routing | Harness/model by Implementation, Manager, QA / Verification, Documentation; task overrides stay on New Task | Models & routing |
| Project / Git | Project repository names; future branch and worktree defaults | Project / Git settings |
| People / stakeholders | Seated user, teams, stakeholder categories and owners | People & stakeholders |
| Notifications | No independent notification preferences exist yet | Future category when notification controls are introduced |

The #35 discovery work belongs under Coding tools. The #36 application routes
belong under Models & routing, while #37 task overrides remain with task
creation so they are visible at the point of use. Future providers add entries
to the shared tool list rather than adding a settings category for each CLI.
No settings data format or save behavior changes as part of this reorganization.
