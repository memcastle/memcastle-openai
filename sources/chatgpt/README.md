# ChatGPT conversation source

The `chatgpt` source mines one document per conversation, without making model calls.
It accepts an official ChatGPT export ZIP with a top-level `conversations.json`, or a standalone `conversations.json`.
Without an export path the source selects the signed-in web account automatically; `mode=web` remains an explicit option.
This is an **experimental** alternative using ChatGPT's undocumented private web interface.
It has been tested with synthetic responses, not a fresh live session; its authentication and endpoints may change.

```sh
memcastle plugin install /path/to/openai-0.1.0.tar.gz --yes
memcastle source enable chatgpt
memcastle mine chatgpt /path/to/chatgpt-export.zip
# Or, after extracting the archive:
memcastle mine chatgpt /path/to/conversations.json
```

Both paths must exist on the daemon's machine.
The source is identified by the canonical path to the export file: replacing that file with a newer export preserves its history.
Choosing a different path creates a different source identity.
Core independently revisits known/pending IDs and discovers export conversations; unchanged content and metadata
are idempotent. Both sources require reconciliation extension `0.1.0` (development Core containing #378 / `c311d09`);
a `0.5.2` version label alone is insufficient.
At most 128 MiB of JSON and 16 MiB per conversation are accepted; split larger exports before importing.
Raw conversation JSON is retained in MemCastle, including branch relationships, message metadata and any attachment/reference descriptors the export contains.
Only text messages by the user and assistant become searchable segments; attachments are not downloaded, and unsupported content is left in raw JSON.
No export can restore conversations already deleted at the origin.

## Experimental live acquisition

The captured desktop app sent both a Bearer token and cookies, but a fresh desktop-compatible PKCE sign-in proved a
renewed bearer alone can read history and projects when curl sends a browser-style User-Agent.
Plain curl and Python's default User-Agent received an HTML 403 edge challenge even with the same bearer; storing a
cookie from `auth.openai.com` does not fix that.
The source declares the tested desktop OAuth client and keeps its refresh token through MemCastle's credential store;
the daemon hands the source a renewed access token for each web call, and no browser cookie is stored.
OpenAI's *documented third-party* Sign in with ChatGPT flow does not grant history access, so this desktop-compatible
private interface may change without notice.
Sign in on the daemon's machine (browser OAuth needs its loopback callback), then mine:

```sh
memcastle source auth chatgpt
memcastle mine chatgpt
```

Export ZIP/JSON imports remain usable while signed out.
The old manual-session environment fallback also remains available when a fresh OAuth sign-in is not possible:

```text
MEMCASTLE_CHATGPT_BEARER=<fresh ChatGPT web access token>
MEMCASTLE_CHATGPT_COOKIE=<fresh ChatGPT web cookie header, if required>
```

Set these outside MemCastle's config and restart the daemon so it inherits them.
An environment bearer takes precedence over OAuth, so remove it to use `source auth chatgpt`.
Do not put them in `memcastle mine` options, project files, a shell history entry or a saved HAR.
The source's process grant runs `curl` with a fixed `https://chatgpt.com` origin, sends headers through standard input, refuses redirects and caps each command's response at 16 MiB.
Installing the plugin shows the `locator` read grant (for a file, its parent directory), `curl` process grant and the two environment-variable grants for consent.
There is currently one registered web account, selected automatically, so no internal account label is needed.
Separate named web accounts are refused until the provider can select them reliably.
An explicit legacy `account=LABEL` remains a non-secret identity label, not an authentication-account selector.
See the root README for the early-0.x reconciliation identity transition; legacy cursor/history ownership is not migrated.

An unfiltered web run reads ordinary and project conversations, including archived projects.
To mine only named projects, use `projects=name:Exact Name`, or `projects=id:PROJECT_ID` for an unambiguous selection.
Separate multiple selectors with commas: `projects=id:ID_A,name:Other Project`.
Names must be exact and unique; a renamed project stops matching, and a newly created project with the old name may
be selected on a later run, so use IDs for unattended miners.
A name containing a comma must use its ID instead.
Project selection stays in options rather than changing account identity. Named miners have independent histories
and checkpoints even with identical account, locator and destination; keep their names stable across filter changes.
An export file does not support `projects` because its project membership has not been verified.
The project sidebar only discovers names and IDs; this source follows the per-project conversation pages for history.
Project membership in raw document metadata comes from those listings, even when conversation details omit it.
Large accounts may need a higher `mining.source_timeout_secs` for discovery; each call remains bounded by both the daemon
setting and this source's 600-second maximum.

Define a persistent miner with the project filter in its saved options:

```sh
memcastle miner set chatgpt-project --source chatgpt \
  --option 'projects=name:Exact Name'
memcastle miner run chatgpt-project
```

The daemon must have `curl` installed.
OAuth tokens are renewed by MemCastle; manually supplied sessions still expire without automatic renewal.
HTTP 401/403 means check the sign-in and session or retry when the private interface changes; HTTP 429 means retry later.
Conversation lists can change during offset paging: the source rejects detected drift and rescans on a later run.
The source revisits the list on each run so edited old conversations are not hidden behind a timestamp watermark.
Message pagination is bounded and fails instead of storing a partial transcript.
The interface may change without notice, including the need for additional headers or a different sign-in.
Validate it with a fresh user-owned session before relying on unattended mining.

## Retention and retirement

Exports are additive editable snapshots: present conversations can update, but omissions, valid empty exports,
and missing export inputs do not establish deletion. Missing/malformed inputs fail without retiring history.
Exports never require web sign-in and cannot be project-filtered.

For web mode, fresh positive project-list membership outside the selected scope explicitly retires a document.
Acquisition refreshes provenance rather than trusting old discovery handles. Listing omission, unknown membership,
archival cleanup, global 401/403, rate limits, network errors and unclassified 404/410 responses are not deletion evidence.
Failed/partial listings fail the job; unresolved membership preserves retained content. A missing selected project
is an error rather than an empty scope. No automatic provider-deletion inference is made from HTTP status alone.

Core registers IDs before acquisition, so failed first reads can recover from inventory without their old handles.
Retirement is repeatable, keeps history, and is reversible when scope/membership permits acquisition again—even with
an unchanged revision. Full runs also reconcile inventory. One-document limits count inventory checks as well as
new candidates; resume limited runs through completed cycles to revisit pending IDs. Empty pages checkpoint without
fake conversations. Bootstrap/update traversal watermarks are not lifecycle exclusion evidence.

## Development

Build the source for `wasm32-wasip2` with Cargo and use `memcastle plugin package` from the provider root;
`memcastle source test sources/chatgpt --no-build` runs JSON, ZIP and empty-export fixtures.
`cargo test --manifest-path sources/chatgpt/Cargo.toml --locked` runs the native export-parser tests, and
`node scripts/smoke_chatgpt.mjs` exercises the
installed source against a stand-in `curl`, not a real account. `node scripts/conformance_web.mjs` runs authenticated
web/empty-project fixtures through the component host with synthetic credentials. `node scripts/smoke_reconciliation.mjs`
checks durable inventory, named-miner isolation, pending recovery, project moves, retirement and restoration in a real daemon.
For a focused smoke test without building the plugin's other modules, build/package this source once and set
`SOURCE_ARCHIVE=sources/chatgpt/dist/chatgpt-0.1.0.tar.gz` when running that script;
it installs the standalone package in the same disposable daemon.
Keep exports, session credentials and traffic captures private: they contain conversation text and account access.
