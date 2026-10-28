import { type GameScan, type SteamGame, listGames } from "@/lib/engine";

// A single scan per session, shared by the home screen and the game screen.
let scan: Promise<GameScan> | null = null;

/** Scan result, started at the first call. `fresh` runs a new scan. */
export function loadGames({ fresh = false } = {}): Promise<GameScan> {
	if (!scan || fresh) {
		scan = listGames();
		// A failure must not stay cached: the next call tries again.
		scan.catch(() => {
			scan = null;
		});
	}
	return scan;
}

/** Updates a game of the in-memory scan (e.g. a changed setting). */
export async function patchGame(appId: number, patch: Partial<SteamGame>) {
	const { games } = await loadGames();
	const game = games.find((g) => g.appId === appId);
	if (game) Object.assign(game, patch);
}

export async function findGame(appId: number): Promise<SteamGame | null> {
	const { games } = await loadGames();
	return games.find((g) => g.appId === appId) ?? null;
}

// Recent games: a simple local convenience, lost without consequence if
// storage is unavailable.
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
		// Storage blocked: do without.
	}
}

/** A game removed from Unalia also leaves the recent list. */
export function forgetRecentGame(appId: number) {
	try {
		localStorage.setItem(RECENTS_KEY, JSON.stringify(recentGameIds().filter((id) => id !== appId)));
	} catch {
		// Storage blocked: do without.
	}
}

/** Games sorted with the recent ones first, in order of use. */
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
