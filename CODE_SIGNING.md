# Code signing policy

Free code signing provided by [SignPath.io](https://about.signpath.io), certificate by [SignPath Foundation](https://signpath.org).

## What is signed

Only the Windows installer and the app it contains, built by the [release workflow](.github/workflows/release.yml) on GitHub Actions from a tagged commit of this repository. Nothing built on a personal machine is signed, and no third-party binary is signed in our name.

Each release is also signed with the updater key, whose public half is in `apps/desktop/src-tauri/tauri.conf.json`: the app refuses any update that doesn't match it.

## Team roles

| Role | Members |
|---|---|
| Committers and reviewers | [Members of the unalia-app organization](https://github.com/orgs/unalia-app/people) |
| Approvers | [Owners of the unalia-app organization](https://github.com/orgs/unalia-app/people?query=role%3Aowner) |

Today the project has a single maintainer, [REVERSS](https://github.com/IIREVERSSII), who holds every role. Changes from other contributors are reviewed before they are merged, and every signing request is approved by hand.

## Privacy

This program will not transfer any information to other networked systems unless specifically requested by the user or the person installing or operating it. The only exception is the update check at startup, which downloads the latest release manifest from GitHub and sends nothing about you.

The services the app talks to, and why, are listed in the [privacy policy](PRIVACY.md).
