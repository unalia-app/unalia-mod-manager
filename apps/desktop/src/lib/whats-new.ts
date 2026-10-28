/**
 * "What's new" after an update: right before installing, the app keeps the
 * version and its notes; on restart on that version, they show once. A
 * simple local convenience: lost without consequence if storage is
 * unavailable.
 */

const KEY = "unalia.whatsNew";

type Pending = { version: string; notes: string };

export function rememberWhatsNew(version: string, notes: string | null) {
	try {
		if (notes?.trim()) localStorage.setItem(KEY, JSON.stringify({ version, notes } satisfies Pending));
	} catch {
		// Storage blocked: do without.
	}
}

/** Notes to show for the running version, only once. */
export function takeWhatsNew(current: string): string | null {
	try {
		const raw = localStorage.getItem(KEY);
		if (!raw) return null;
		localStorage.removeItem(KEY);
		const pending = JSON.parse(raw) as Partial<Pending>;
		// Update that didn't go through: nothing to announce.
		return pending.version === current && typeof pending.notes === "string" ? pending.notes : null;
	} catch {
		return null;
	}
}
