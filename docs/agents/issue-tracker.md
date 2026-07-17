# Issue tracker: GitHub

Issues and PRDs for this repository live in GitHub Issues at
`liumingjian/ape-dts`. Use the `gh` CLI for all operations.

## Conventions

- Create: `gh issue create --title "..." --body "..."`
- Read: `gh issue view <number> --comments`
- List: `gh issue list --state open`
- Comment: `gh issue comment <number> --body "..."`
- Label: `gh issue edit <number> --add-label "..."`
- Close: `gh issue close <number> --comment "..."`

Run these commands inside the repository so `gh` infers the repository from
the Git remote.

## Pull requests as a triage surface

PRs as a request surface: no.

## Skill conventions

When a skill says “publish to the issue tracker”, create a GitHub issue.

When a skill says “fetch the relevant ticket”, use:

`gh issue view <number> --comments`

A bare `#42` can refer to an issue or pull request. Resolve it with
`gh pr view 42`, falling back to `gh issue view 42`.
