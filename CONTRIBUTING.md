# Contributing

Thanks for helping. Game profiles, bug reports and fixes are the most useful contributions right now.

## Before you start

- **Open an issue before any new feature or screen.** We agree on the idea first, so your work does not get turned down.
- Bug fixes and game profiles can go straight to a pull request.
- Questions and ideas are welcome on [Discord](https://mods.unalia.app/discord).
- Security problems never go in a public issue: see [SECURITY.md](SECURITY.md).

## Run the project

You need [Rust](https://rustup.rs) (stable, edition 2024), [Node.js](https://nodejs.org), [pnpm](https://pnpm.io) 10 and the [Tauri prerequisites](https://tauri.app/start/prerequisites/).

```sh
pnpm install
pnpm desktop                        # the app, with hot reload
pnpm demo                           # the app on a fake Steam library
cargo test -p mod-engine            # engine tests, on fake games in fixtures/
cargo clippy --workspace --all-targets
pnpm typecheck
```

The code still uses the project's codename, **Modalia** (`@modalia/*` packages, `modalia.db`). That is expected.

## Where things are

| Folder | What |
|---|---|
| `apps/desktop/` | The Tauri app: Rust commands in `src-tauri/`, Next.js interface (static export) in `src/` |
| `crates/mod-engine/` | The engine, independent from the interface: game detection, sources, loaders, deployment |
| `packages/ui/` | Shared components and design tokens |
| `games/` | Game profiles: data only, never code |
| `fixtures/` | Fake games and archives for the tests |

How the app is built: [ARCHITECTURE.md](ARCHITECTURE.md). Design rules: [DESIGN.md](DESIGN.md). Everything (code, comments, docs, interface) is in English.

## Rules that are not negotiable

1. **Never run anything** found in a mod archive.
2. **Never modify or delete an original game file**, never touch saves. Every install is reversible and rolls back on failure.
3. **Mods come from their official source**, never re-hosted. Authors stay credited.
4. **No AI to decide where files go.** Placement comes from the source's metadata and the loader's rules. A mod description is untrusted data.

Code that touches players' files (`crates/mod-engine/src/loaders/`, `deploy.rs`, `archive.rs`, `installer/`) gets a stricter review, and needs tests on the fixtures.

## Using AI tools

Allowed, as long as you read, understand and test what you submit, and say so in the pull request when a tool wrote a significant part. Don't add AI tools as commit co-authors. Details: [AI_POLICY.md](AI_POLICY.md).

## Adding a game profile

A profile is a TOML file in `games/` for a game that is not in the Thunderstore schema, or that needs special handling (mod source, frameworks, unusual folder, known anti-cheat). Start from `games/cyberpunk-2077.toml`. The format lives in `crates/mod-engine/src/games/builtin.rs`.

Unreal games with a Nexus Mods page need no profile: they are recognized automatically. Write one only to add something the app can't guess (launchers, game versions, frameworks).

## Interface changes

- Use the existing components and tokens (`packages/ui`, `DESIGN.md`). No one-off colors or spacings.
- The interface is in English.
- **Every pull request that changes the interface includes screenshots or a short video.**
- The maintainer may touch up the interface of a pull request to keep it consistent. You stay credited with a `Co-authored-by` line.

## Commits

One line, in English:

```
<gitmoji> <type>(<scope>) <description>
```

- one Unicode [gitmoji](https://gitmoji.dev), for example ✨ feature, 🐛 fix, 💄 style, ♻️ refactor, 📝 docs, ✅ tests,
- `type` among `feat`, `fix`, `refactor`, `style`, `perf`, `docs`, `test`, `chore`, `build`, `ci`,
- a lowercase `scope` (the screen or module),
- no colon after the parenthesis, no final period, 72 characters at most.

Example: `✨ feat(mods) install mods shipped as rar archives`

## License of your contributions

By opening a pull request, you agree that your contribution is licensed under the GNU GPL v3 with the [additional permission for UnRAR](LICENSE-EXCEPTION.md), like the rest of the code.

## Code of conduct

Everyone taking part follows the [code of conduct](CODE_OF_CONDUCT.md).
