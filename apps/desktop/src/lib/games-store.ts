import { type GameScan, type SteamGame, listGames } from "@/lib/engine";

// Un seul scan par session, partagé par l'accueil et l'écran du jeu.
let scan: Promise<GameScan> | null = null;

/** Résultat du scan, lancé au premier appel. `fresh` relance un scan. */
export function loadGames({ fresh = false } = {}): Promise<GameScan> {
	if (!scan || fresh) {
		scan = listGames();
		// Un échec ne doit pas rester en cache : le prochain appel réessaie.
		scan.catch(() => {
			scan = null;
		});
	}
	return scan;
}

export async function findGame(appId: number): Promise<SteamGame | null> {
	const { games } = await loadGames();
	return games.find((g) => g.appId === appId) ?? null;
}

// Jeux récents : simple confort local, perdu sans conséquence si le
// stockage est indisponible.
const RECENTS_KEY = "modalia.recentGames";
const RECENTS_MAX = 8;

export function recentGameIds(): number[] {
	try {
		const ids: unknown = JSON.parse(localStorage.getItem(RECENTS_KEY) ?? "[]");
		return Array.isArray(ids) ? ids.filter((id): id is number => typeof id === "number") : [];
	} catch {
		return [];
	}
}

export function rememberGame(appId: number) {
	try {
		const ids = [appId, ...recentGameIds().filter((id) => id !== appId)].slice(0, RECENTS_MAX);
		localStorage.setItem(RECENTS_KEY, JSON.stringify(ids));
	} catch {
		// Stockage bloqué : on s'en passe.
	}
}

/** Jeux triés avec les récents en tête, dans l'ordre d'utilisation. */
export function withRecentsFirst(games: SteamGame[]): SteamGame[] {
	const recents = recentGameIds();
	const rank = (g: SteamGame) => {
		const i = recents.indexOf(g.appId);
		return i === -1 ? recents.length : i;
	};
	return [...games].sort((a, b) => rank(a) - rank(b));
}

export function gameHref(appId: number): string {
	return `/game/?id=${appId}`;
}

export function modHref(appId: number, modId: string): string {
	return `/mod/?game=${appId}&id=${encodeURIComponent(modId)}`;
}
