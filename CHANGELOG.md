# Changelog

All notable changes to Unalia Mod Manager. The format follows [Keep a Changelog](https://keepachangelog.com), and versions follow [Semantic Versioning](https://semver.org).

## [Unreleased]

## [0.1.0] - 2026-10-27

First public beta, Windows only.

### Added

- Steam games detected automatically, other games added by picking their folder.
- Thunderstore source, with mods placed by the rules of each game's loader (BepInEx, MelonLoader, Shimloader, UMM, Northstar…).
- Nexus Mods source for Cyberpunk 2077, with free or Premium accounts: choose which files of a mod to install, frameworks installed from their authors' GitHub, conflicts named, warning after a game update.
- Unreal Engine adapter (`.pak` mods, UE4SS), prepared for Phantom Blade Zero.
- One-click install with dependencies and loader, parallel and resumable downloads.
- My mods: turn on and off, remove with Undo, updates on your decision.
- Play button, with changes queued while the game runs.
- Accent color and ambience taken from each game's cover.
- `.zip`, `.7z` and `.rar` archives.
- Links to support mod authors.
- Go back to one of the last 3 versions of a mod in one click from My mods, without downloading it again.
- Remove a game added by hand: its mods leave the game folder, the game itself stays installed.
- Reset mods for a game: Unalia takes out its mods and sends mod files left by other tools (added by hand, Vortex…) to the Recycle Bin, so you can start clean.
- The app updates itself: a new version is announced when the app opens and in Settings > Support, and installs only when you click Update. Each update is signed and checked before it runs.
- Settings > Support lists the licenses of the open source projects the app is built on, and its own GNU GPL v3.
