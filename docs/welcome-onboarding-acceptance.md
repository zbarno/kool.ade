# Welcome workspace setup acceptance

This acceptance contract covers workspace setup for #431. Existing local
checkouts and embedded planning data are not supported setup inputs. The Welcome
screen offers a GitHub code-repository clone or a new blank project; saved
managed workspaces reopen through their recorded association.

## Clone a code repository with its planning repository

1. The user enters a GitHub URL for the code repository. The optional project
   label only changes the display name; it cannot select a local checkout.
2. Review shows the exact code repository, derived same-owner
   `<code-repository>.packet` planning repository, managed local code
   destination, and the work Kool.ad/e will perform.
3. Kool.ad/e clones the code repository into its managed checkout location.
   It derives the planning repository; there is no planning-repository picker.
4. If the planning repository exists, Kool.ad/e clones and validates it. If it
   is missing, Kool.ad/e creates it with the code repository's GitHub
   visibility, initializes the planning workspace, and uses that separate
   repository for all project artifacts.
5. A visibility mismatch or unsafe existing local destination stops setup with
   an actionable error. Setup does not alter visibility, overwrite an existing
   checkout, or create a second workspace identity.
6. After the paired repositories and saved association validate, Kool.ad/e
   opens the project board. A later launch reopens that saved workspace.

There is no embedded-packet migration, discovery, or fallback. Planning files
are read and written only in the separate planning repository. Code and
planning checkouts cannot be the same path or contain one another.

## Manual acceptance still required

The operator will test the final candidate on Pop!_OS with COSMIC. The operator
is comfortable with terminal commands. The separate novice-session requirement
was explicitly waived; no novice result is claimed. Use the concise script in
[welcome-onboarding-usability.md](welcome-onboarding-usability.md) and record
the candidate commit, outcomes, confusion and actionable findings there. Keep
#431 open for the broader onboarding acceptance that this clone flow does not
cover.

## Validation evidence

Completion requires real egui interaction coverage, local Git and GitHub pair
setup coverage, failure and retry safety, an independent review of the final
diff, all four repository quality gates, required hosted checks and the
operator's manual walkthrough. Screenshots must use synthetic data and must not
include private paths or credentials.
