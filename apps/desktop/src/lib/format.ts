const compact = new Intl.NumberFormat("fr-FR", { notation: "compact", maximumFractionDigits: 1 });
const relative = new Intl.RelativeTimeFormat("fr-FR", { numeric: "auto" });

/** 10716544 → « 10,7 M ». */
export function compactNumber(n: number): string {
	return compact.format(n);
}

const UNITS: [Intl.RelativeTimeFormatUnit, number][] = [
	["year", 365 * 24 * 3600],
	["month", 30 * 24 * 3600],
	["week", 7 * 24 * 3600],
	["day", 24 * 3600],
	["hour", 3600],
	["minute", 60],
];

/** Date ISO → « il y a 3 jours ». */
export function relativeDate(iso: string, now = Date.now()): string {
	const seconds = (new Date(iso).getTime() - now) / 1000;
	for (const [unit, size] of UNITS) {
		if (Math.abs(seconds) >= size) return relative.format(Math.round(seconds / size), unit);
	}
	return "à l'instant";
}
