/**
 * « What's new » après une mise à jour : juste avant d'installer, l'appli
 * garde la version et ses notes ; à la relance sur cette version, elles
 * s'affichent une fois. Simple confort local : perdu sans conséquence si
 * le stockage est indisponible.
 */

const KEY = "unalia.whatsNew";

type Pending = { version: string; notes: string };

export function rememberWhatsNew(version: string, notes: string | null) {
	try {
		if (notes?.trim()) localStorage.setItem(KEY, JSON.stringify({ version, notes } satisfies Pending));
	} catch {
		// Stockage bloqué : on s'en passe.
	}
}

/** Notes à montrer pour la version qui tourne, une seule fois. */
export function takeWhatsNew(current: string): string | null {
	try {
		const raw = localStorage.getItem(KEY);
		if (!raw) return null;
		localStorage.removeItem(KEY);
		const pending = JSON.parse(raw) as Partial<Pending>;
		// Mise à jour qui n'est pas allée au bout : rien à annoncer.
		return pending.version === current && typeof pending.notes === "string" ? pending.notes : null;
	} catch {
		return null;
	}
}
