# Architecture

How Unalia Mod Manager is built, for contributors. The design rules live in [DESIGN.md](DESIGN.md).

## Principles

These decide every trade-off. A change that breaks one of them won't be merged.

1. **The user has nothing to do.** No folder to pick, no file to copy, no instruction to read. If a step can be automated, it is.
2. **We never break a game.** No original game file is modified or deleted. Saves are never touched. Every install is reversible and transactional: it fully succeeds or rolls back.
3. **We never run anything.** The app copies and links files. It never launches an `.exe`, a script or an installer found in a mod archive.
4. **We respect modders.** Mods are always downloaded from their official source, never re-hosted. Authors are credited and their page is one click away.
5. **Design is a feature.** Every state (loading, progress, error, empty) is designed. No generic screen, no raw error code, no popup that breaks the flow.
6. **One adapter per family of games, never one per game.** A loader adapter unlocks every game that uses it. Game-specific knowledge is data (`games/*.toml`), not code.

## Stack

- **Desktop shell:** Tauri 2. Windows only for now.
- **Interface:** Next.js as a **static export** (`output: "export"`), React, TypeScript, Tailwind CSS 4. No server components, server actions, API routes or middleware: everything that touches the system goes through Tauri commands.
- **Engine:** Rust (`crates/mod-engine`), independent of the interface.
- **Local state:** SQLite.
- **Monorepo:** pnpm workspaces and Turborepo.

## Repository

```
apps/desktop/           Tauri app: Rust commands in src-tauri/, Next.js interface in src/
crates/mod-engine/      The engine: everything that knows about games, mods and files
packages/ui/            Shared components and design tokens
games/                  Game profiles: data only, open to the community
fixtures/               Fake Steam libraries, games and archives for the tests
scripts/                Demo library, license file, updater manifest
```

`apps/` holds what runs or ships; `packages/` and `crates/` hold what is shared.

## Interface and engine

- The interface calls the engine through **Tauri commands** (`invoke("install_mod", {...})`), declared in `apps/desktop/src-tauri/src/` (`lib.rs`, `catalog.rs`, `mods.rs`) and mirrored in `apps/desktop/src/lib/engine.ts`.
- The engine pushes progress (download, install) and game state (running, pending changes) through **Tauri events**.
- The interface stays "dumb": no file logic in the front end. It only sends ids, never paths: game folders always come from the engine's own scan.
- Mod descriptions are untrusted: their HTML is parsed and sanitized (allowlist, HTTPS only) before rendering, because the webview can call engine commands.

## Engine blocks (`mod-engine`)

| Block | Role |
|---|---|
| **detect** | Reads Steam's `libraryfolders.vdf` and `appmanifest_*.acf` files to list games and their folders, without a heavy disk walk. Identifies the engine by file signatures (`UnityPlayer.dll` for Unity, `Engine/` plus `.pak` files for Unreal…) and spots known anti-cheats. Also reads and writes Steam's non-Steam shortcuts. |
| **games** | What the app knows about each game: Thunderstore's ecosystem schema (cached on disk), hand-written profiles (`games/*.toml`, embedded in the binary), and Unreal games recognized automatically from their name on Nexus Mods (`games/auto.rs`). Pure data, no code. |
| **sources** | One catalog per source, translated into a neutral mod format (`ModSummary`, `ModDetail`): **Thunderstore**, **Hexium** (Thunderstore-compatible, searched in memory since it has no search API, falls back to Thunderstore for the mods it doesn't host), **Nexus Mods** (GraphQL v2 for search, REST v1 with the user's key for downloads, `nxm://` links for free accounts, SSO login) and **GitHub** releases (frameworks and authors who publish there). |
| **resolver** | From "I want this mod" to an ordered install plan: dependencies first, each package once, a single loader, cycles refused. |
| **loaders** | Where each file of a package goes, by family: Thunderstore schema rules (BepInEx, MelonLoader, Shimloader…), REDengine (Cyberpunk 2077), Unreal (`.pak`, LogicMods, UE4SS), Bethesda (`Data/`, `plugins.txt`). Frameworks (RED4ext, UE4SS…) are described by the game profile and installed from their authors' GitHub release. Loaders are code: few of them, reviewed with extra care. |
| **download** | Parallel chunked downloads with resume, cancel, disk space check and SHA-256 hash. |
| **archive** | Safe extraction of `.zip`, `.7z` and `.rar`: no path escaping the target, no links, no archive bombs, nothing left half extracted. |
| **store** | Mods stored without duplicates, like pnpm's store: each file once under its SHA-256, each extracted version described by a "path to hash" package. Previous versions stay, to go back without downloading again. |
| **deploy** | Mods live in the app's data folder and are **hard linked** into the game folder (copied across drives). Config files are always copied, and the player's edits are kept. Every placed file and created folder is recorded, so removal brings the folder back exactly as it was. |
| **state** | SQLite: installed mods, versions and previous versions, enabled state, deployments, edited configs, pending changes, settings. |
| **installer** | Ties it all together for one click: resolve, download, extract, store, place, deploy, record. The whole plan is transactional. |
| **launch** | Launches the game (through Steam or its executable), watches its process, and queues changes while it runs: they are applied when it closes. |
| **foreign** | Finds mod files left in a game by other tools (by hand, Vortex…), so "Reset mods" can send them to the Recycle Bin. |
| **artwork** | Game images from Steam's local cache, and the accent color extracted from the cover (OKLCH, contrast checked). |
| **advice** | Turns an error chain into one human sentence: what happened, then what to do. |

## Security

- **Mods:** official APIs only, over HTTPS; the source's hash is checked when it publishes one, ours is always kept. A GitHub copy of a Nexus file is only used if Nexus recognizes its hash as that exact file.
- **Install:** nothing from an archive is ever executed; extraction is hardened; original game files and saves are never modified; games with an anti-cheat get a clear ban warning.
- **App:** the webview has no file access (Tauri capabilities), external links open in the system browser over HTTPS only, updates are signed and verified with the public key in `tauri.conf.json`.
- **Community:** game profiles are data, never code. Changes to `loaders/`, `deploy.rs`, `archive.rs` and `installer/` get a stricter review and need tests on the fixtures.

## Tests

- The engine is tested end to end on fake games (`fixtures/`) against fake Thunderstore, Hexium, Nexus and GitHub servers that serve real archives: install, uninstall, enable and disable, rollback on failure, booby-trapped archives, resumed downloads.
- `cargo test -p mod-engine` runs them. Tests against the real APIs are ignored by default: `cargo test -p mod-engine --test live -- --ignored`.
- Real-game checks happen on Windows before each release.
