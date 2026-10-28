# Game profiles

One file per game missing from the Thunderstore schema, or with special cases: mod source, frameworks, unusual folder, known anti-cheat.

Profiles are **pure data**: they never contain or run code. Format: see `cyberpunk-2077.toml` and `crates/mod-engine/src/games/builtin.rs`. Unreal games with a Nexus Mods page don't need a profile: they are recognized automatically from `auto/unreal.toml`.
