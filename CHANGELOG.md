# Changelog

All notable changes to Unalia Mod Manager. The format follows [Keep a Changelog](https://keepachangelog.com), and versions follow [Semantic Versioning](https://semver.org).

## [Unreleased]

### Added

- My Mods: when the mods were last checked for updates, with a button to check again.
- My Mods: a Deprecated badge on a mod its author marked as deprecated, hid or removed.
- My Mods: an Off first sort brings the mods you turned off to the top.
- My Mods: select several mods, or a range with Shift, to turn them on or off or remove them at once, with one Undo.
- My Mods: a compact layout puts the mods in columns across the window, to see more of a long list at once.
- A failed install offers to ask for help on Discord.
- Help & Feedback says where to follow up after sending, as the form can't get you a reply.
- Import a mod list: a link to the mod lists shared on Discord.

### Fixed

- Nexus Mods: a game with more than 20 Nexus mods has them all checked for updates, not only 20 of them.
- Changing the game's version or resetting it checks for updates again right away.

## [0.1.6] - 2026-11-01

### Added

- Nexus Mods: a Copy link next to the page to open, for when Windows can't open it (no default browser).
- My Mods: search your mods by name or author, and sort them by name or by last installed or updated.
- Discover: filter a game's mods by category (Weapons, Gameplay, Utilities...).
- Mod page: the mod's changelog, when its author wrote one.
- An invitation to the Unalia Discord after a mod installs, shown once, and a link to it in What's new.
- Mods marked as adult content now show up in Discover, with their images blurred. Settings > Adult content picks how strongly, or turns the blur off. Clicking an image on a mod's page shows it as is.

### Changed

- A mod's page has a Back to mods button, like the one in Settings.
- Discover: the category filter is a list you can search, with a cross to clear it.
- Notifications have a new look. Their details show right away, and one you can undo tells how long it stays, with a click to keep it open.

### Fixed

- Coming back from a mod's page returns to the tab you left, My Mods included, instead of Discover, with your search, category and sort still there.
- A mod's page says when a dependency is already installed, instead of asking to install it.
- Import a mod list: a long mod name stays inside the window.
- Import a mod list: the list shows the dependencies too and says how many mods it found. They still come with their mods, in the version made for your game.

## [0.1.5] - 2026-10-31

### Added

- Share your mod list with a code, and import a friend's to play with the same mods.
- Cyberpunk 2077: REDmod mods load. When one is on, Play deploys them and starts the game with `-modded`, and Unalia says when the free REDmod DLC is missing.
- Unreal Engine games: proxy DLLs, ASI plugins, ReShade and UE4SS C++ mods go where they must.
- Unreal Engine games: `Engine.ini` mods are merged into the player's settings.
- Unreal Engine games: ReShade presets install, and Unalia says when ReShade is missing.
- Help & Feedback: a bug report attaches a report, on by default, so the team can understand the bug. Turn it off for a message if you prefer.
- My Mods: a button opens a mod's Nexus Mods page.

### Changed

- My Mods: Update all is always there, greyed out when everything is up to date.
- My Mods uses more of the window, and the file and variant pickers are wider.
- A name cut short slides on hover to show the rest, in My Mods, on mod cards and in the pickers.

### Fixed

- Nexus Mods: every request names the app and its version, as the Nexus API policy asks (only some did).
- Nexus Mods: a file that is a tool or a cheat table says so instead of being treated as a mod.
- Nexus Mods: code blocks in descriptions show `\` and other escaped characters instead of `&#92;`.
- Long versions in the version picker stay on one line.
- Two mods that place the same file can now be installed together, like a patch and the mod it fixes, or mods that ship the same library. The last one installed has its file in the game, except that a mod required by another goes under it whatever the install order, an update keeps its place, and the other mod's file comes back when the winner is turned off or removed. My Mods shows which mod replaces what.
- Mods a mod really needs and Unalia can't install by itself stay in view under it, in My Mods and on its page, one click from each page, until they are installed. Requirements their author marks as optional or as an add-on are left out.
- Nexus Mods: installing an optional file from the site adds it to the mod instead of replacing the mod, and installing the main file keeps it. A mod is only offered an update it would really install.

## [0.1.4] - 2026-10-29

### Added

- Mod page: a row of screenshots, taken from the main picture and the images of the description, with arrows when they don't all fit. Click one, the mod's picture or an image in the description to see it full screen and browse with the arrows.
- Nexus Mods: when a download holds the mod in several variants, one folder each (Skip Intro, Skip Intro + Instant Menu), Unalia asks which one to install. Updates keep it.

### Fixed

- The Discord link opens the server again: the old invite had expired.
- Nexus Mods: updating a mod whose file name holds its version (`PBZSkipIntro 1.0.0`, then `1.1.0`) installs the new file instead of the old one again.
- Nexus Mods: descriptions no longer show `[/*]` at the end of list items.

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
