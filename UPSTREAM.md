# Downstream maintenance

This repository is a private downstream of [`openai/codex`](https://github.com/openai/codex).

## Scope

- Keep Codex's agent runtime, authentication, configuration, and session formats compatible with upstream.
- Keep local product changes isolated to `codex-rs/tui/` whenever possible.
- Land each local TUI feature as a separate commit with focused snapshot coverage.
- Do not modify the installed Codex binary in place. Build and select this downstream binary explicitly so the stock CLI remains a rollback target.

## Remotes

- `origin`: this private repository.
- `upstream`: `https://github.com/openai/codex.git`.

The initial downstream base is upstream commit `7769bccbb2b4e9469a36b12510e73594fa03c5d5`.

## Upstream update policy

`.github/workflows/upstream-sync.yml` checks `upstream/main` every four hours and opens or refreshes a pull request when new commits are available. It never merges updates into `main` automatically. The workflow can also be run immediately through GitHub Actions' `workflow_dispatch`.

For every sync pull request:

1. Review the upstream release notes and the merge diff.
2. Run the affected TUI tests and inspect any snapshot changes.
3. Merge only after the downstream TUI behaviour has been verified.

If Git reports conflicts, the workflow aborts the merge and creates one open issue named `Upstream sync blocked`. Resolve the conflict in a dedicated branch; do not force-push `main` or discard local TUI commits.

## Rollback

Every local UI feature must remain an independent commit. Revert the specific feature commit or run the stock `codex` binary if a downstream build is faulty. Do not use `git reset --hard` on `main` as a rollback mechanism.
