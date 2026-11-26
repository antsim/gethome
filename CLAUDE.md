# CLAUDE.md

Työaikalaskuri: a personal macOS menu bar app (Tauri 2 + Rust + Angular) that
shows how much of today's working time is left, based on Toggl. Both Toggl
2.0 (`focus.toggl.com`) and Toggl Track (API v9) are supported; the API is
chosen from the token (`toggl_sk_…` = Toggl 2.0).

**[PLAN.md](PLAN.md) is the source of truth** for requirements, API usage,
the request budget and the implementation phases. Read it before making
changes. Chapter 12 lists decisions made during implementation.

## Commands

```bash
npm install                 # once
npm run dev                 # run the app (Angular dev server + Tauri)
npm test -- --watch=false   # Angular unit tests (Vitest)
npm run format:check        # Prettier
npm run bundle              # release .app + .dmg (macOS)

cd src-tauri
cargo test                  # Rust unit + mocked-HTTP tests
cargo fmt --check
cargo clippy --all-targets -- -D warnings
```

The Rust crate embeds the built frontend, so `cargo` commands that compile
the app need `dist/` to exist: run `npm run build` once first.

## Architecture

The Rust backend owns all state and timing; Angular is only a view.

```
src-tauri/src/
  lib.rs              Tauri setup: plugins, tray, windows, command registration
  service.rs          AppService: wires everything; engine loop + UI loop tasks
  commands.rs         Thin #[tauri::command] wrappers around AppService
  error.rs            AppError → { code, message } (Finnish message for the UI)
  clock.rs            Clock trait (SystemClock, ManualClock for tests)
  tray.rs             Tray title formatting + menu
  windows.rs          Floating timer window (+ summary size) and settings window
  engine/
    calendar.rs       Local-day bounds (DST-safe)
    state.rs          DayState built from time entries, SyncStatus
    compute.rs        PURE: (DayState, now, tz, targets) → TimerViewModel
    summary.rs        PURE: day's tasks rounded to half hours (timesheet copy)
    scheduler.rs      SyncEngine: polling, backoff, wake/day change, stop/resume
  toggl/
    client.rs         The only place that talks HTTP to Toggl: auth, rate
                      budget, error mapping, flavor dispatch (Track / 2.0)
    track.rs          Toggl Track API v9 endpoints + wire format
    focus.rs          Toggl 2.0 endpoints + wire format (org/workspace scoped)
    models.rs         Product-independent domain models used by the engine
    rate_budget.rs    Sliding 1 h request counter
  settings/
    mod.rs            Settings model + validation
    persistence.rs    tauri-plugin-store (settings.json in app data dir)
    secrets.rs        API token in macOS Keychain (keyring)

src/app/
  core/               Models, typed invoke wrappers, state services (signals)
  timer/              Floating window view (+ pure timer-view.ts mapping)
  summary/            Day summary shown in the expanded floating window
  settings/           Settings window (reactive forms)
```

Data flow: `SyncEngine` publishes an `EngineSnapshot` via a `watch` channel →
the UI loop runs `compute()` every second → emits `timer-state` and sets the
tray title. Commands that change Toggl state go to the engine loop through an
`mpsc` channel so that they are serialized with polling.

Both windows load the same Angular app; `App` picks the view from the Tauri
window label (`timer` / `settings`).

**CSP gotcha:** Angular injects component styles as `<style>` tags at
runtime. In bundled builds Tauri adds nonces to the CSP, which makes browsers
ignore `'unsafe-inline'`, so `tauri.conf.json` keeps
`dangerousDisableAssetCspModification: ["style-src"]`. Angular's critical CSS
inlining is disabled in `angular.json` because it relies on an inline script.
`npm run dev` does not show these problems; verify UI changes with
`npx tauri build --debug --no-bundle` too.

## Conventions

- **Rust**: `cargo fmt` and `cargo clippy --all-targets -- -D warnings` must
  be clean. Errors use `thiserror`. No `unwrap()`/`expect()` in production
  code (tests are fine). Recover poisoned mutexes with
  `unwrap_or_else(|e| e.into_inner())`.
- **Toggl**: every HTTP call goes through `TogglClient::send` so it is
  recorded in the rate budget. Product-specific wire formats stay in
  `track.rs` / `focus.rs`; the engine only sees `toggl/models.rs`.
  Toggl 2.0 API docs: https://engineering.toggl.com/docs/focus/ (OpenAPI
  spec linked there). Normal use must stay ≤ 20 requests/hour (there is a test
  for this in `engine/scheduler/tests.rs`).
- **Pure logic**: `compute.rs`, `calendar.rs` and `state.rs` do no I/O and
  never read the clock; `now` is a parameter. `SyncEngine` gets time from an
  injected `Clock`.
- **Angular**: standalone components, signals, `OnPush`, no NgModules, strict
  TypeScript and strict templates. Components call the backend only through
  `TauriCommands`.
- **Language**: UI texts (including user-facing error messages in
  `error.rs`) are in Finnish; code, comments and logs are in English.
- **Wire format**: Rust structs sent to the UI use
  `#[serde(rename_all = "camelCase")]`; keep `src/app/core/models.ts` in sync.

## Secrets

The repository is public. Never commit API tokens or other secrets. The
Toggl token lives only in the macOS Keychain; settings are stored in the app
data directory, outside the repository. Tests use fake tokens against
`wiremock`.

## Adding features

- New Toggl endpoint: add a public method to `TogglClient` that dispatches to
  `track_*` / `focus_*` implementations (+ wiremock tests for both), and call
  it from `SyncEngine` or `AppService`, never from elsewhere.
- New setting: add the field to `Settings` (with a default, `#[serde(default)]`
  keeps old files loading), validate in `Settings::validate`, mirror it in
  `models.ts` and the settings form.
- New data in the view: extend `TimerViewModel` in `compute.rs` with a unit
  test, then `models.ts` and `timer-view.ts`.
