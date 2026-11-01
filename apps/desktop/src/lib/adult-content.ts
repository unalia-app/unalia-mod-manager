import { type CSSProperties, useSyncExternalStore } from "react";

export type AdultContentSettings = {
	/** Their images are blurred. */
	blur: boolean;
	/** In pixels. */
	strength: number;
};

export const BLUR_MIN = 4;
export const BLUR_MAX = 40;
export const BLUR_STEP = 4;

const DEFAULTS: AdultContentSettings = { blur: true, strength: 16 };

const KEY = "modalia.adultContent";

function read(): AdultContentSettings {
	try {
		const stored: unknown = JSON.parse(localStorage.getItem(KEY) ?? "{}");
		return { ...DEFAULTS, ...(stored as Partial<AdultContentSettings>) };
	} catch {
		return DEFAULTS;
	}
}

let current: AdultContentSettings | null = null;
const listeners = new Set<() => void>();

function adultContent(): AdultContentSettings {
	current ??= read();
	return current;
}

export function setAdultContent(change: Partial<AdultContentSettings>) {
	current = { ...adultContent(), ...change };
	try {
		localStorage.setItem(KEY, JSON.stringify(current));
	} catch {
		// Storage blocked: the choice holds for this session.
	}
	for (const listener of listeners) listener();
}

export function useAdultContent(): AdultContentSettings {
	return useSyncExternalStore(
		(listener) => {
			listeners.add(listener);
			return () => listeners.delete(listener);
		},
		adultContent,
		() => DEFAULTS,
	);
}

/** The strength, as a style. */
export function blurStyle(strength: number): CSSProperties {
	return { filter: `blur(${strength}px)` };
}

/** Style of a mod's image: blurred when it is an adult mod and the setting asks for it. */
export function useAdultBlur(adult: boolean): CSSProperties | undefined {
	const { blur, strength } = useAdultContent();
	return adult && blur ? blurStyle(strength) : undefined;
}
