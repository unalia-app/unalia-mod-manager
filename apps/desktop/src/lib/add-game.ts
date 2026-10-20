import { isTauri } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";

import { addManualGame } from "@/lib/engine";
import { loadGames, rememberGame } from "@/lib/games-store";

/**
 * Demande le dossier d'un jeu, le fait reconnaître par le moteur et
 * rafraîchit la liste des jeux. `null` si l'utilisateur a annulé.
 */
export async function pickAndAddGame(): Promise<number | null> {
	if (!isTauri()) return null;
	const path = await open({ directory: true, title: "Pick your game's folder" });
	if (typeof path !== "string") return null;
	const appId = await addManualGame(path);
	await loadGames({ fresh: true });
	rememberGame(appId);
	return appId;
}
