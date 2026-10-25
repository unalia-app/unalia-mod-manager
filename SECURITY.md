# Security policy

## Reporting a vulnerability

Please do not open a public issue for a security problem.

Email **gaeltbch.pro@gmail.com** with:

- what you found and where (file, command, or screen),
- how to reproduce it,
- the impact you expect (for example, a mod writing outside the game folder).

You will get an answer within 7 days. Once a fix is released, you will be credited in the changelog, unless you prefer to stay anonymous.

## Supported versions

Only the latest release gets security fixes.

## Scope

The most sensitive parts are the ones that touch your files:

- extracting mod archives (`.zip`, `.7z`, `.rar`): paths escaping the target folder, links, archive bombs,
- deploying mods into the game folder and restoring it on removal,
- handling `nxm://` links and downloads.
