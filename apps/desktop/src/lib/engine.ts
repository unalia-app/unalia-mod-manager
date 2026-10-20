import { invoke, isTauri } from "@tauri-apps/api/core";

// Miroir des types sérialisés par le moteur (serde, camelCase).
export type Engine = "unity" | "unreal" | "unknown";

/** Ce que Thunderstore sait du jeu (brique games du moteur). */
export type GameProfile = {
	id: string;
	name: string;
	community: string;
	steamAppIds: number[];
	loader: string;
	supported: boolean;
	coverUrl: string | null;
	heroUrl: string | null;
};

export type SteamGame = {
	appId: number;
	name: string;
	installDir: string;
	library: string;
	engine: Engine;
	profile: GameProfile | null;
	/** Décidé par le moteur : profil Thunderstore avec un loader géré. */
	moddable: boolean;
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

export function isModdable(game: SteamGame): boolean {
	return game.moddable;
}

export const ENGINE_LABEL: Record<Engine, string> = {
	unity: "Unity",
	unreal: "Unreal",
	unknown: "Moteur inconnu",
};

const LOADER_LABEL: Record<string, string> = {
	bepinex: "BepInEx",
	melonloader: "MelonLoader",
	"recursive-melonloader": "MelonLoader",
	shimloader: "Shimloader",
	godotml: "GodotML",
};

/** Sous-titre d'un jeu : moteur, et loader quand Thunderstore le connaît. */
export function gameSubtitle(game: SteamGame): string {
	const loader = game.profile && (LOADER_LABEL[game.profile.loader] ?? game.profile.loader);
	return loader && game.profile?.loader !== "none" ? `${ENGINE_LABEL[game.engine]} · ${loader}` : ENGINE_LABEL[game.engine];
}

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

// Mods (brique sources du moteur, format neutre SPEC §6.5).

export type ModSort = "popular" | "newest" | "updated";

export type ModSummary = {
	id: string;
	source: string;
	name: string;
	author: string;
	summary: string;
	iconUrl: string | null;
	categories: string[];
	downloads: number;
	updatedAt: string;
	pinned: boolean;
	deprecated: boolean;
};

export type ModDependency = { id: string; minVersion: string };

export type ModVersion = {
	number: string;
	publishedAt: string;
	downloadUrl: string;
	sha256: string | null;
	size: number | null;
	dependencies: ModDependency[];
};

export type Mod = Omit<ModSummary, "iconUrl" | "downloads" | "pinned"> & {
	images: string[];
	downloads: number | null;
	sourceUrl: string;
	websiteUrl: string | null;
	versions: ModVersion[];
};

export type Page<T> = { items: T[]; total: number; hasMore: boolean };

export function searchMods(community: string, text: string, sort: ModSort, page: number): Promise<Page<ModSummary>> {
	return invoke("search_mods", { community, text, sort, page });
}

export function getMod(community: string, id: string): Promise<Mod> {
	return invoke("get_mod", { community, id });
}

/** Markdown brut de la source : à nettoyer avant tout affichage (SPEC §7). */
export function modDescription(id: string, version: string): Promise<string> {
	return invoke("mod_description", { id, version });
}
