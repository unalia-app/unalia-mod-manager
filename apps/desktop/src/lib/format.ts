const compact = new Intl.NumberFormat("en-US", { notation: "compact", maximumFractionDigits: 1 });
const relative = new Intl.RelativeTimeFormat("en-US", { numeric: "auto" });

/** 10716544 → « 10.7M ». */
export function compactNumber(n: number): string {
	return compact.format(n);
}

/** 10737418240 → « 10.0 GB ». */
export function bytes(n: number): string {
	const units = ["B", "KB", "MB", "GB", "TB"];
	let value = n;
	let unit = 0;
	while (value >= 1024 && unit < units.length - 1) {
		value /= 1024;
		unit += 1;
	}
	return unit === 0 ? `${n} B` : `${value.toFixed(value >= 100 ? 0 : 1)} ${units[unit]}`;
}

/** 380 → « 6 min left ». Arrondi grossier : une estimation, pas un chrono. */
export function timeLeft(seconds: number): string {
	if (!Number.isFinite(seconds) || seconds < 0) return "";
	if (seconds < 60) return `${Math.max(5, Math.round(seconds / 5) * 5)} s left`;
	const minutes = Math.round(seconds / 60);
	if (minutes < 60) return `${minutes} min left`;
	const hours = Math.floor(minutes / 60);
	return `${hours} h ${minutes % 60} min left`;
}

const UNITS: [Intl.RelativeTimeFormatUnit, number][] = [
	["year", 365 * 24 * 3600],
	["month", 30 * 24 * 3600],
	["week", 7 * 24 * 3600],
	["day", 24 * 3600],
	["hour", 3600],
	["minute", 60],
];

/** Date ISO → « 3 days ago ». */
export function relativeDate(iso: string, now = Date.now()): string {
	const seconds = (new Date(iso).getTime() - now) / 1000;
	for (const [unit, size] of UNITS) {
		if (Math.abs(seconds) >= size) return relative.format(Math.round(seconds / size), unit);
	}
	return "just now";
}
