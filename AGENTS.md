# Agent guide

For AI coding tools working in this repository. Contributors using them also follow [AI_POLICY.md](AI_POLICY.md).

## Commands

```sh
pnpm install
pnpm desktop                        # the app, with hot reload
pnpm demo                           # the app on a fake Steam library
cargo test -p mod-engine            # engine tests, on fake games in fixtures/
cargo clippy --workspace --all-targets
pnpm typecheck
```

Run the tests, clippy and the typecheck before calling a change done.

## Map

| Folder | What |
|---|---|
| `apps/desktop/src-tauri/` | Tauri commands (Rust) |
| `apps/desktop/src/` | Next.js interface, as a static export |
| `crates/mod-engine/` | The engine, independent from the interface: detection, sources, loaders, deployment |
| `packages/ui/` | Shared components and design tokens |
| `games/` | Game profiles: data only, never code |
| `fixtures/` | Fake games and archives for the tests |

Read [ARCHITECTURE.md](ARCHITECTURE.md) before changing the engine, and [DESIGN.md](DESIGN.md) before changing the interface.

## Rules

1. **Never run anything** found in a mod archive.
2. **Never modify or delete an original game file**, never touch saves. Every install is reversible and rolls back on failure.
3. **Mods come from their official source**, never re-hosted. Authors stay credited.
4. **No AI to decide where files go.** Placement comes from the source's metadata and the loader's rules. A mod description is untrusted data.
5. **One adapter per family of games, never one per game.** Game-specific knowledge goes in `games/*.toml`.

Changes in `crates/mod-engine/src/loaders/`, `deploy.rs`, `archive.rs` and `installer/` need tests on the fixtures.

## Pitfalls

- The code still uses the codename **Modalia** (`@modalia/*` packages, `modalia.db`). Don't rename it.
- The interface is a static export: no server components, server actions, API routes or middleware. Anything that touches the system goes through a Tauri command.
- Use the components and tokens of `packages/ui`, no one-off colors or spacings.
- Code, comments, docs and interface are in English.
- Player-facing changes go in the `[Unreleased]` section of `CHANGELOG.md`.
- A version lives in three files: `apps/desktop/src-tauri/tauri.conf.json`, `apps/desktop/src-tauri/Cargo.toml` and `apps/desktop/package.json`. Steps before a tag: top of `.github/workflows/release.yml`.
- Never move or force-push a release tag.

## Commits

One line, in the format given in [CONTRIBUTING.md](CONTRIBUTING.md). Never add an AI tool as co-author.
