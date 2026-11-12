# Codex history source

The `codex` source reads one document per `rollout-*.jsonl` transcript under `~/.codex/sessions/YYYY/MM/DD/`.
It is a sandboxed WebAssembly component and is separate from the Codex integration, which configures live MCP use.

It has read-only access to the selected locator and the default sessions directory, plus `HOME` to locate that directory.
It has no network, process, write or credential permission.

The source files user and assistant messages plus tool-call names.
It deliberately excludes model reasoning, tool output, command/file contents, injected context and unrecognised record types.

After installing the plugin and reviewing consent for both sources:

```sh
memcastle source enable codex
memcastle mine codex
memcastle mine codex /backups/codex/sessions
memcastle miner set codex-project --source codex --option project=/work/project
memcastle miner run codex-project
```

## Project scope and reconciliation

Requires development Core containing reconciliation #378 (`c311d09`); a `0.5.2` version label alone is insufficient.
The optional `project` path includes sessions whose session-metadata working directory equals that path or is
a descendant. Matching is lexical and component-based: `/work/app` excludes `/work/application`. Historical
directories need not still exist; symlinks are not resolved. Relative CLI path options are resolved by Core;
the adapter requires an absolute path. Missing/conflicting cwd metadata is unknown membership and is deferred
under a project filter. This option does not grant access to the project directory.

Core owns durable known/pending inventory and isolates each named miner. Discovery traverses rollouts by
modification time and relative path; inventory independently checks existing IDs for edits, including backdated
edits. Use a full run to discover newly introduced files behind the watermark. Limits count inventory checks
as well as discovery candidates; repeated limited runs resume persisted traversal. Empty filtered pages checkpoint
without fake documents. Project filters do not change source identity, and only the selected miner reconciles its scope.

A complete rollout positively outside a selected project is explicitly retired; old stored cwd may also prove
exclusion after a filter change when a rollout has disappeared. Retirement is repeatable and restoring scope
reacquires the same ID even when its revision has not changed. File rotation/disappearance alone never retires history.
Blank, malformed, changing or non-conversational snapshots defer acquisition without replacing retained content.
Complete shorter rewrites are edits, not deletions. Stable reads and JSONL validation detect observable incomplete
writes, but Codex supplies no completion marker for distinguishing every well-formed intermediate rewrite.
Unexpected read failures fail the job. Pending first acquisitions remain retryable through Core's inventory.

See the root README for the early-0.x named-miner identity transition and retained legacy histories.
