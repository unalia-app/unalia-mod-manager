# Privacy

**Unalia Mod Manager collects nothing.** There is no account, no analytics, no telemetry and no Unalia server.

## What stays on your computer

- The list of your games, read from your Steam files or added by you.
- The mods you installed, their files and their state, in a local database.
- Your settings (interface size, recent games).
- Your Nexus Mods API key, if you connect your account.
- A log file of what the app did and the errors it met, kept small (`logs` folder).

None of it is sent anywhere. **Settings > Support > Copy report** puts a report in your clipboard for a bug report: your versions, games, mods and recent logs, without your account name, API key or Windows user name. You decide where to paste it.

## Who the app talks to

Only the services that host the games' mods and images, to do what you ask:

| Service | Why |
|---|---|
| Thunderstore | Browse and download mods |
| Nexus Mods | Browse and download mods, with your API key if you connected it |
| GitHub | Download frameworks and mods published there by their authors |
| Steam (CDN) | Game covers that are not in your local Steam cache |

Each request identifies the app (`UnaliaModManager/<version>`), as these services ask. They see your IP address, like any website you visit, and apply their own privacy policies.

Links you open (a mod page, a donation link) open in your browser.

## Uninstalling

Your data lives in the app data folder (`%LOCALAPPDATA%\app.unalia.modmanager` on Windows). Delete it to remove everything the app stored.
