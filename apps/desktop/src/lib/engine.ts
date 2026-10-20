import { invoke, isTauri } from "@tauri-apps/api/core";

// Miroir des types sérialisés par le moteur (serde, camelCase).
export type Engine = "unity" | "unreal" | "unknown";

export type SteamGame = {
	appId: number;
	name: string;
	installDir: string;
	library: string;
	engine: Engine;
};

export type GameScan = {
	steamFound: boolean;
	games: SteamGame[];
};

export async function listGames(): Promise<GameScan> {
	// Hors de Tauri (ex. `next dev` dans un navigateur), aucun moteur n'est joignable.
	if (!isTauri()) return { steamFound: false, games: [] };
	return invoke<GameScan>("list_games");
}

/** Le prototype ne gère que BepInEx, donc les jeux Unity (SPEC §5). */
export function isModdable(game: SteamGame): boolean {
	return game.engine === "unity";
}

export const ENGINE_LABEL: Record<Engine, string> = {
	unity: "Unity",
	unreal: "Unreal",
	unknown: "Moteur inconnu",
};
