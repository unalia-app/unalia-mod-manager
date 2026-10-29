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
| Hexium | Browse and download mods, for the games it hosts |
| Nexus Mods | Browse and download mods, with your API key if you connected it. Recognize Unreal games by their name |
| GitHub | Download frameworks and mods published there by their authors. Check for a new version of the app at startup |
| Steam (CDN) | Game covers that are not in your local Steam cache |

Each request identifies the app (`UnaliaModManager/<version>`), as these services ask. They see your IP address, like any website you visit, and apply their own privacy policies.

Links you open (a mod page, a donation link) open in your browser.

## Uninstalling

Your data lives in the app data folder (`%LOCALAPPDATA%\app.unalia.modmanager` on Windows).

When you uninstall the app on Windows:

- **"Delete the application data" checked:** the uninstaller first removes every mod from your games, so each game folder is back to how it was. Then it deletes the app data folder.
- **Unchecked:** your mods stay in your games and keep working. Install the app again and it finds them.

Delete the app data folder by hand only after removing your mods in the app: without it, the app can't tell which files it placed.
