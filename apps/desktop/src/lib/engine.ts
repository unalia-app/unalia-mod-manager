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

export type ArtworkKind = "cover" | "hero" | "header" | "logo";

/**
 * Image du cache local de Steam, servie par le protocole `artwork` du moteur.
 * Sous Windows, la webview expose les protocoles maison en `http://<nom>.localhost`.
 */
export function artworkUrl(appId: number, kind: ArtworkKind): string {
	const windows = typeof navigator !== "undefined" && navigator.userAgent.includes("Windows");
	const base = windows ? "http://artwork.localhost" : "artwork://localhost";
	return `${base}/${appId}/${kind}`;
}

const accents = new Map<number, Promise<string | null>>();

/** Couleur d'accent CSS du jeu (`null` : accent neutre). Calculée une fois. */
export function gameAccent(appId: number): Promise<string | null> {
	if (!isTauri()) return Promise.resolve(null);
	let accent = accents.get(appId);
	if (!accent) {
		accent = invoke<string | null>("game_accent", { appId }).catch(() => null);
		accents.set(appId, accent);
	}
	return accent;
}
