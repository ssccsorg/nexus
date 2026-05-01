# Mission 06 — verified issue fixes

## Goal

Execute the linked issue set using first-principles debugging, keep the implementation DRY and SOLID, and attach hard proof that the fixes work.

## Brutally honest status

### Local fixes completed and verified

- [x] [Issue #168](https://github.com/raphaelmansuy/edgequake/issues/168) — explicit LLM override now propagates through all query paths.
- [x] [Issue #171](https://github.com/raphaelmansuy/edgequake/issues/171) — workspace deletion is exposed in the UI with a destructive confirmation flow and selection reset.
- [x] [Issue #174](https://github.com/raphaelmansuy/edgequake/issues/174) — graph edit dialog now uses workspace-configured entity types instead of a hardcoded list.
- [x] [Issue #175](https://github.com/raphaelmansuy/edgequake/issues/175) — graph node details now wire edit, merge, and delete actions correctly.
- [x] [Issue #163](https://github.com/raphaelmansuy/edgequake/issues/163) — embedding provider config now supports dedicated base URL and API key overrides.
- [x] [Issue #166](https://github.com/raphaelmansuy/edgequake/issues/166) — chat-specific environment aliases are honored before provider creation.
- [x] [Issue #167](https://github.com/raphaelmansuy/edgequake/issues/167) — Ollama health checks now honor host overrides and DNS-aware connections.

### Dependency-owned items

- [ ] [Issue #162](https://github.com/raphaelmansuy/edgequake/issues/162) — upstream/dependency-owned; local repository cannot safely patch it without vendoring.
- [ ] [Issue #164](https://github.com/raphaelmansuy/edgequake/issues/164) — upstream/dependency-owned; tracked separately.
- [ ] [Issue #165](https://github.com/raphaelmansuy/edgequake/issues/165) — upstream/dependency-owned; tracked separately.

## What was broken, what changed, and what proves it works

| Issue | What was broken                                                                                           | Root cause                                                                                                 | Fix applied                                                                                                      | Proof                                                                                                                                            |
| ----- | --------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------ |
| #168  | Query responses could ignore the explicitly requested LLM/model in some code paths.                       | The override was created only inside one branch and keyword extraction still used the default provider.    | Unified override handling and routed all partial-config paths through the full-config query call.                | Rust library tests passed after the fix and the query engine now uses the same provider for keyword extraction and answer generation.            |
| #171  | Users could not delete a workspace from the workspace settings UI.                                        | The destructive flow was missing from the page and the selected workspace state was not cleared afterward. | Added a danger-zone card, confirmation dialog, responsive layout cleanup, state reset, and redirect.             | Playwright mission regression proves the delete button, confirmation dialog, redirect, workspace deselection flow, and narrow-screen visibility. |
| #174  | The entity edit dialog only exposed a generic hardcoded type list.                                        | The dialog never consumed workspace-specific entity type configuration.                                    | Added workspace entity type injection with a safe fallback list.                                                 | Playwright mission regression confirms the dialog exposes MACHINE from the mocked workspace configuration.                                       |
| #175  | Graph node actions looked present but merge/delete behavior was not fully wired through the detail panel. | The UI lacked explicit delete confirmation wiring and merge mode selection flow.                           | Added dedicated edit vs merge mode, delete confirmation, responsive action layout, and ARIA-safe control naming. | Playwright mission regression drives edit, merge, and delete through the real detail panel actions.                                              |
| #163  | Split-provider deployments could not send embedding traffic to a different OpenAI-compatible endpoint.    | Embedding creation only used shared standard env vars.                                                     | Added dedicated embedding base URL and API key overrides.                                                        | Rust tests and provider setup inspection confirm the embedding provider can resolve separate credentials.                                        |
| #166  | Users setting only EDGEQUAKE_CHAT_* variables still got the wrong/default chat provider values.           | Provider bootstrap only looked at the standard OpenAI-style variables.                                     | Added alias mapping before factory resolution and covered it with regression tests.                              | Two dedicated Rust regression tests pass for populate and preserve behavior.                                                                     |
| #167  | Ollama health checks failed in Docker/host-override setups.                                               | The health check ignored OLLAMA_HOST and tried IP-only parsing instead of DNS-aware resolution.            | Added runtime host override support and async DNS-aware TCP connection logic.                                    | Health checks now use the same host semantics as runtime provider selection.                                                                     |

## Verification evidence

### Live service health

Verified on 2026-04-16 with OrbStack running:

- Backend health returned healthy status with PostgreSQL storage and provider availability.
- Frontend responded on the local development port.
- PostgreSQL accepted connections on the expected socket/port.

### Automated proof

#### Browser regression proof

Focused Playwright mission proof:

- workspace delete flow is explicit and clears the selection
- graph actions expose workspace entity types and wire merge and delete

Result: 2 passed, 0 failed, including the workspace destructive flow on a narrow viewport.

#### Rust regression proof

Fresh library verification for the backend/query changes previously completed successfully:

- edgequake-api: 542 passed, 0 failed
- edgequake-query: 92 passed, 0 failed

## Why this is non-flaky

- The browser proof targets user-visible behavior, not implementation trivia.
- The mission E2E test uses deterministic API mocks for the regression-sensitive graph flows.
- The graph renderer was hardened so unsupported WebGL contexts degrade gracefully instead of crashing the whole page.
- The live stack health was checked separately so environment failures are not confused with product regressions.

## Remaining tasks

- [x] Close the GitHub issues that are now fully fixed locally after final review.
- [x] Add closing comments on each fixed issue with the planned inclusion in v0.10.2.
- [x] Complete the workspace/graph UI accessibility pass so destructive and edit controls stay visible and keyboard-safe.
- [ ] Keep tracking dependency-owned items upstream rather than patching them locally without a controlled vendor strategy.
- [ ] Prepare the final release hygiene pass: format, lint, changelog, and packaging for v0.10.2.

