# Settings information architecture

Settings have one home, accessible directly from the desktop header or from
Workspace → Settings. The desktop sidebar groups pages by purpose. Narrow or short
windows use a page selector above the content, and each page has one scrollbar.

| Group | Page | Controls |
| --- | --- | --- |
| Application | General | Planner persona, save and restore |
| Application | Appearance | Reduced motion |
| Work & Tools | Coding Tools | Available tools, default tool, executable paths, setup guidance and rediscovery |
| Work & Tools | Models & Routing | Tool and model by Implementation, Manager, QA / Verification and Documentation |
| Work & Tools | Automation | Worker capacity, automatic planning/building/publication, checks and queue recovery |
| Project | Project & Git | Primary code repository, published-spec ref, local workspace identity |

The Workspace menu groups workspace switching and refresh separately from
project resources. Disconnect is at the bottom. Configuration stays in
Settings, rather than duplicate menu toggles.

Task-specific routing overrides remain under Advanced routing in New Task.
The local user, Git authentication, consent, approval, and QA gates remain.
Closing an embedded Project page closes Settings and discards its unsaved draft.
