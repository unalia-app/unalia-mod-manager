# Unalia Mod Manager

**Pick your mods. We handle the rest.** A one-click mod manager for PC games: no folder to find, no file to copy, no install guide to read.

[![Latest release](https://img.shields.io/github/v/release/unalia-app/unalia-mod-manager?label=release)](https://github.com/unalia-app/unalia-mod-manager/releases/latest)
[![License: GPL v3](https://img.shields.io/badge/license-GPL%20v3-blue)](LICENSE)
[![Discord](https://img.shields.io/badge/chat-Discord-5865F2?logo=discord&logoColor=white)](https://discord.gg/AMtDVqygh5)

**[Download for Windows](https://github.com/unalia-app/unalia-mod-manager/releases/latest/download/unalia-mod-manager-setup.exe)** · [Website](https://mods.unalia.app) · [Discord](https://discord.gg/AMtDVqygh5)

> **Beta.** Windows 10 and 11 only for now. Expect rough edges, and please [report them](https://github.com/unalia-app/unalia-mod-manager/issues).

## What it does

- **Finds your games.** Reads your Steam libraries in a second. Games outside Steam (GOG, Epic, standalone) can be added by picking their folder, and added to Steam for its overlay.
- **Installs in one click.** The mod, its dependencies and the loader or framework it needs (BepInEx, UE4SS, RED4ext…), in the right order.
- **Lets you change your mind.** Turn a mod off without uninstalling it, remove it with an Undo, go back to a previous version without downloading it again.
- **Keeps you up to date, on your terms.** Updates are shown, never forced: a working setup stays working.
- **Waits for your game.** Changes made while the game runs are queued and applied when you close it.
- **Cleans up after other tools.** "Reset mods" removes what was installed by hand or by another manager, to the Recycle Bin.
- **Warns you about anti-cheats.** Games with Easy Anti-Cheat or BattlEye get a clear warning: mods stay your call, but online they can get you banned.
- **Looks like it belongs to the game.** The accent color and ambience follow the cover of the game you are modding.

## Supported games

| Source | Games | Loaders and frameworks |
|---|---|---|
| [Thunderstore](https://thunderstore.io) | Every game listed there (Hollow Knight: Silksong, Lethal Company, Valheim…) | BepInEx, MelonLoader, Shimloader, UMM, Northstar… |
| [Nexus Mods](https://www.nexusmods.com) | Cyberpunk 2077 | Cyber Engine Tweaks, RED4ext, redscript, ArchiveXL, TweakXL, Codeware |
| Nexus Mods | Unreal Engine games, recognized on their own by their name on Nexus (Phantom Blade Zero, Hogwarts Legacy…) | `.pak` mods, LogicMods, UE4SS |
| Nexus Mods | Skyrim Special Edition, Fallout 4 | Plugins, archives and loose files, `plugins.txt` kept in sync |

Archives can be `.zip`, `.7z` or `.rar`. Nexus Mods works with a free account: the app sends you to the mod's page for the "Mod Manager Download" click Nexus requires, then takes over.

Next on the list: script extenders and FOMOD installers for Bethesda games, more loaders (RE Engine, Godot) and more sources. See the [roadmap](ROADMAP.md).

## Why it is safe

- **Nothing is ever run.** The app copies and links files. It never launches an `.exe`, a script or an installer found in a mod.
- **Your game files are never modified.** Mods are linked into the game folder, never written over original files. Your saves are never touched.
- **Everything is reversible.** Every file placed is recorded. Remove all your mods, or uninstall the app with its data, and the game folder is back to how it was. An install either fully succeeds or rolls back.
- **Mods come from their official source.** Downloaded over HTTPS from Thunderstore, Nexus Mods or the author's GitHub, never re-hosted. Authors are credited and their page is one click away.
- **Signed updates.** The app checks every update against its public key before installing it.
- **Open source.** Read the code, build it yourself.

Details: [SECURITY.md](SECURITY.md) and [PRIVACY.md](PRIVACY.md).

## Build from source

You need [Rust](https://rustup.rs) (stable, edition 2024), [Node.js](https://nodejs.org) and [pnpm](https://pnpm.io) 10, plus the [Tauri prerequisites](https://tauri.app/start/prerequisites/) for Windows.

```sh
pnpm install
pnpm desktop                  # runs the app with hot reload
pnpm demo                     # the app on a fake Steam library
cargo test -p mod-engine      # engine tests, on fake games
```

How it is built: [ARCHITECTURE.md](ARCHITECTURE.md). Design rules: [DESIGN.md](DESIGN.md).

## Contributing

Bug reports, game profiles and fixes are welcome. Read [CONTRIBUTING.md](CONTRIBUTING.md) first, and open an issue before starting a new feature or screen. Using AI tools? See the [AI policy](AI_POLICY.md). Questions and ideas: [Discord](https://discord.gg/AMtDVqygh5).

## The Unalia family

Unalia Mod Manager is a sibling of [Unalia](https://unalia.app), the game backlog. Same visual DNA, separate apps.

## Credits

Made by [REVERSS](https://github.com/IIREVERSSII).

Mods and their metadata come from [Thunderstore](https://thunderstore.io), [Nexus Mods](https://www.nexusmods.com) and the authors' GitHub releases. Each mod belongs to its author, credited in the app with a link to its page. Game artwork comes from Steam and belongs to the games' publishers.

The app is built on [Tauri](https://tauri.app), [Next.js](https://nextjs.org), [React](https://react.dev) and many other open source projects. `.rar` archives are read with UnRAR by Alexander Roshal. Every license is in [THIRD_PARTY_LICENSES.md](THIRD_PARTY_LICENSES.md), also shipped with the app (Settings > Support).

## License

The code is under the [GNU GPL v3](LICENSE), with an [additional permission](LICENSE-EXCEPTION.md) to combine it with UnRAR. The Unalia names and logos are not: see [TRADEMARKS.md](TRADEMARKS.md).
