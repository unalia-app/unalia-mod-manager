# Changelog

All notable changes to Unalia Mod Manager. The format follows [Keep a Changelog](https://keepachangelog.com), and versions follow [Semantic Versioning](https://semver.org).

## [Unreleased]

## [0.1.4] - 2026-10-29

### Added

- Mod page: a row of screenshots, taken from the main picture and the images of the description, with arrows when they don't all fit. Click one, the mod's picture or an image in the description to see it full screen and browse with the arrows.

### Fixed

- The Discord link opens the server again: the old invite had expired.
- Nexus Mods: updating a mod whose file name holds its version (`PBZSkipIntro 1.0.0`, then `1.1.0`) installs the new file instead of the old one again.

## [0.1.3] - 2026-10-29

### Fixed

- Help & Feedback: messages now reach the team. Before, they were lost after the "Thanks" screen.

## [0.1.2] - 2026-10-29

### Fixed

- Game covers: when the cover published by Thunderstore doesn't download (Valheim), the one from the game's Steam page is used instead of no cover at all.
- Nexus Mods: a mod whose version is written differently on its page and on its file (`1.0` and `1.0.0`) is no longer offered as an update.
- Nexus Mods: cancelling an install that waits for its turn stops it and removes it from the list right away.

## [0.1.1] - 2026-10-29

### Added

- Hexium source (hexium.gg) for the games it hosts, Valheim first: a Thunderstore / Hexium switch in Discover. Dependencies published only on Thunderstore come from there, and each mod updates from the source it was installed from.
- Skyrim Special Edition and Fallout 4 (Steam), with mods from Nexus Mods: plugins, archives and loose files go to `Data/`, and the game's `plugins.txt` follows the mods you turn on. Script extenders and mods with an installer that asks for options (FOMOD) come next.
- Unreal Engine games with a Nexus Mods page are recognized on their own, from Steam or added by hand, with UE4SS when a mod needs it.
- Games with an anti-cheat (Easy Anti-Cheat, BattlEye) warn that mods can get you banned online, on their page and once before you start.
- Uninstall a mod from its page or its card, with Undo.
- Uninstall the app from Settings > Support.
- Refresh button in the game library, and a refresh when the window comes back: a game installed while the app is open shows up.
- A new version announces itself in a window with its notes, and "What's new" opens after the update.

### Changed

- New notifications, stacked at the bottom of the window: hover to keep them, swipe to dismiss.
- Bigger game cover on the game's page.

### Fixed

- Phantom Blade Zero: UE4SS goes next to the game's real executable (`platform/steam`), in the build made for the game, so its Lua mods start.

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
