# Työaikalaskuri

A personal macOS app that shows how much of today's working time is left,
in the menu bar and in a small floating window. The data comes from
[Toggl](https://toggl.com/) (Toggl 2.0 or Toggl Track). When no Toggl timer
is running, the countdown stops and the app shows **"Tauolla"** (on a break).

```
⏱ 3:42:15            menu bar, timer running
⏸ 3:42 Tauolla       no timer running
✓ +0:12              target reached, 12 min of overtime
```

> The app's user interface is in Finnish. This README gives the Finnish
> labels with English translations in parentheses.

## Features

- Time left in the menu bar. It updates every second, also while the window is hidden.
- A frameless, always-on-top window. You can drag it to an external display, for example, and it remembers its position.
- Stop and resume the Toggl timer directly from the app.
- When you pause with less than 15 minutes of the day's target left, the
  entry ends at the moment the target is reached instead of now. You can
  shut down a little early and still have a full day. If you resume before
  that time, the entry ends when you resume instead (Toggl Track; with
  Toggl 2.0 the new entry simply starts).
- A daily target per weekday (default Mon–Fri 7:30, Sat–Sun 0 h).
- Estimated finishing time ("valmis klo 16:05", done at 16:05) and break length.
- Light and dark theme, adjustable opacity, launch at login.
- Fewer than 20 Toggl API requests per hour in normal use. Rate limits, network outages and sleep are handled gracefully.
- The API key is stored only in the macOS Keychain.

## Installation

Requirements: macOS 11+, [Node.js](https://nodejs.org/) 22+, [Rust](https://rustup.rs/) (stable) and the Xcode Command Line Tools.

```bash
git clone https://github.com/antsim/gethome.git
cd gethome
npm install
```

Run in development mode:

```bash
npm run dev
```

Build a release version (`.app` and `.dmg`):

```bash
npm run bundle
```

The bundle is written to `src-tauri/target/release/bundle/`. The app is not
signed, so the first time you open it, right-click it in Finder and choose
*Open*.

### Setting the API key

The app supports both **Toggl 2.0** (`focus.toggl.com`) and the older
**Toggl Track** (`track.toggl.com`). The product is detected from the key
automatically.

- **Toggl 2.0:** *Settings → Toggl account → API key*. The key starts with
  `toggl_sk_` and is shown only once. Creating a new key invalidates the
  old one.
- **Toggl Track:** *API Token* on your
  [profile page](https://track.toggl.com/profile).

1. Open the app's menu in the menu bar and choose *Asetukset…* (Settings).
2. Paste the key and click *Tallenna* (Save). The app verifies the key and
   shows the connected account and workspace.

The key is stored only in the Keychain. It is never written to a file or
sent anywhere other than the Toggl API.

With Toggl 2.0, the app reads the entries of one workspace: the one Toggl
gives as the default for the key (`users/me/landing-workspace`). Break
entries (*break*) and planned-only entries do not count as working time.

## Usage

| Action | How |
|---|---|
| Show/hide the window | Menu bar → *Näytä ikkuna* / *Piilota ikkuna* (Show / Hide window) |
| Stop the timer / resume work | The ⏸/▶ button in the window, or the menu bar |
| Refresh now | Menu bar → *Päivitä nyt* (Refresh now; at most once every 30 seconds) |
| Settings | Menu bar → *Asetukset…*, or ⚙ in the window (shown when the pointer is over the window) |
| Move the window | Drag it from anywhere |

If you stop the timer in the Toggl app, the change shows up here within
four minutes. This is because of Toggl API rate limits (30 requests per
hour on the Free plan). Changes made through this app show up immediately.

## Troubleshooting

**`failed to run 'cargo metadata' … No such file or directory`**
Rust is missing or `cargo` is not on your PATH. Install Rust and load its
environment:

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
```

```bash
source "$HOME/.cargo/env"
```

```bash
cargo --version
```

Also make sure the Xcode Command Line Tools are installed (`xcode-select --install`).

**`npm warn allow-scripts …`**
Recent npm versions do not run package install scripts without approval.
The warning usually does not block development. If the build complains
about esbuild, approve its script and reinstall:

```bash
npm approve-scripts esbuild
```

```bash
npm install
```

## Development

```bash
npm test -- --watch=false     # Angular tests (Vitest)
npm run format:check          # Prettier
npm run build                 # Angular build (needed before cargo commands)

cd src-tauri
cargo test                    # Rust tests (computation + mocked Toggl API)
cargo clippy --all-targets -- -D warnings
cargo fmt --check
```

Run the `cargo` commands in the `src-tauri` directory, where `Cargo.toml` is.

On Linux, compiling the Rust code needs Tauri's
[system dependencies](https://tauri.app/start/prerequisites/#linux)
(for example `libwebkit2gtk-4.1-dev`). The app itself is supported and
tested only on macOS.

The architecture and code conventions are described in
[CLAUDE.md](CLAUDE.md). The original plan and the decisions made during
implementation are in [PLAN.md](PLAN.md) (in Finnish).

### Project structure

```
src/              Angular UI (floating window and settings)
src-tauri/        Rust backend: state, timing, Toggl client, tray
PLAN.md           Implementation plan (in Finnish)
CLAUDE.md         Architecture and conventions
```

## Privacy

- The app talks only to the Toggl API (`focus.toggl.com` or `api.track.toggl.com`).
- The API key is kept only in the macOS Keychain.
- Settings (targets, display options, and the Toggl account details and workspace) are stored in the app's data directory (`~/Library/Application Support/io.github.antsim.tyoaikalaskuri/`).

## License

[MIT](LICENSE)
