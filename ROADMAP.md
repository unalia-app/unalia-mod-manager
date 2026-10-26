# Roadmap

What comes after the first release, in order of priority. Plans change with your feedback: the most requested games decide the next loader or source.

## Next (v0.2)

- **Less disk space** (done). Files stored once, by their fingerprint (like pnpm's store), and going back to a previous version of a mod in one click, without downloading it again.
- **Smaller updates.** Download only the files that changed in a mod.
- **Storage clean-up, part two.** Keep a store per drive so games outside `C:` still use hard links.
- **More safety.** A warning when a placed file changes outside the app, trust signals for very new mods from unknown authors, and known anti-cheat games blocked or flagged.
- **Conflicts by priority.** When two mods ship the same file, choose which one wins, without drivers or injection.
- **Nexus Mods, continued.** "Log in with Nexus", API key in the system keychain, more games, collections, and never taking over `nxm://` links from Vortex or Mod Organizer 2 without asking.
- **Remove a game added by hand** (done).
- **Reset a game's mods** (done). Start clean when another tool left mods behind.
- **Signed installer and signed updates.**

## Loaders

One adapter per family of games, never one per game.

| Family | Games | Status |
|---|---|---|
| Thunderstore schema rules (BepInEx, MelonLoader, Shimloader, UMM, Northstar) | most Thunderstore games | done |
| REDengine | Cyberpunk 2077 | done |
| Unreal: `.pak` mods, UE4SS, LogicMods | Phantom Blade Zero, then Hogwarts Legacy… | done (profile per game) |
| Archive placed as is (GodotML) | Brotato… | planned |
| Return of Modding, Lovely, GDWeave, Rivet | a few games each | planned |
| RE Engine (Fluffy, REFramework) | Monster Hunter Wilds, Resident Evil | later |
| Bethesda (SKSE, load order, FOMOD) | Skyrim, Fallout | last |

## Later

- Restore your mods in one click after reinstalling or updating a game.
- A "vanilla" launch: everything off for one session, back on in one click.
- Profiles: several setups per game.
- Shareable mod lists and modpacks.
- Community game themes.
- Creator tools: publish once, available everywhere.
- Link with [Unalia](https://unalia.app), the game backlog.
- Epic, GOG and other launchers.
