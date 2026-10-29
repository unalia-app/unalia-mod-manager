# Code signing policy

The Windows installer is not code signed yet. Windows SmartScreen may warn about an unknown app, and a few antivirus engines that rely on heuristics may flag it. Once a code signing certificate is in place, this page will say who issued it.

## How to check the installer

- **Get it from the source.** Download it only from [GitHub Releases](https://github.com/unalia-app/unalia-mod-manager/releases) or from [mods.unalia.app](https://mods.unalia.app), which links there.
- **Compare the hash.** GitHub shows the SHA-256 of every release file. In PowerShell, run `Get-FileHash .\unalia-mod-manager-setup.exe` and compare.
- **See how it was built.** Every installer is built by the [release workflow](.github/workflows/release.yml) on GitHub Actions from a tagged commit of this repository. Nothing built on a personal machine is published.
- **Build it yourself.** The steps are in the [README](README.md#build-from-source).

## Signed updates

Each release is signed with the updater key, whose public half is in `apps/desktop/src-tauri/tauri.conf.json`: the app refuses any update that doesn't match it. This applies today, with or without a code signing certificate.

## Team roles

| Role | Members |
|---|---|
| Committers and reviewers | [Members of the unalia-app organization](https://github.com/orgs/unalia-app/people) |
| Approvers | [Owners of the unalia-app organization](https://github.com/orgs/unalia-app/people?query=role%3Aowner) |

Today the project has a single maintainer, [REVERSS](https://github.com/IIREVERSSII), who holds every role. Changes from other contributors are reviewed before they are merged, and every release is tagged by hand.

## Privacy

This program will not transfer any information to other networked systems unless specifically requested by the user or the person installing or operating it. The only exception is the update check at startup, which downloads the latest release manifest from GitHub and sends nothing about you.

The services the app talks to, and why, are listed in the [privacy policy](PRIVACY.md).
