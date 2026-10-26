# Additional permission for UnRAR

Unalia Mod Manager is licensed under the GNU General Public License version 3 (see `LICENSE`), with the additional permission below. It applies to every file of this repository covered by that license.

The app reads `.rar` archives with UnRAR, the RAR decompression code by Alexander Roshal, bundled through the `unrar_sys` crate. The UnRAR license forbids using its code to recreate the RAR compression algorithm. That restriction is not compatible with the GPL, so this permission allows the two to be combined.

## Additional permission under GNU GPL version 3 section 7

If you modify this Program, or any covered work, by linking or combining it with UnRAR (or a modified version of that library), containing parts covered by the terms of the UnRAR license, the licensors of this Program grant you additional permission to convey the resulting work. Corresponding Source for a non-source form of such a combination shall include the source code for the parts of UnRAR used as well as that of the covered work.

## What this means

- The code of Unalia Mod Manager stays under the GPL v3. UnRAR keeps its own license, reproduced in `THIRD_PARTY_LICENSES.md`.
- You may distribute the app, or a fork of it, with UnRAR inside. Its other GPL obligations still apply.
- When you redistribute a modified version, you may keep this permission or remove it, as section 7 of the GPL allows.
- Contributions to this repository are accepted under the GPL v3 with this permission (see `CONTRIBUTING.md`).
