import { type ClassValue, clsx } from "clsx";
import { extendTailwindMerge } from "tailwind-merge";

// tailwind-merge doit connaître les tokens maison pour ne pas confondre
// `text-fg-muted` (couleur) et `text-sm` (taille).
const twMerge = extendTailwindMerge({
	extend: {
		theme: {
			color: [
				"bg",
				"surface",
				"surface-hover",
				"surface-strong",
				"line",
				"line-strong",
				"fg",
				"fg-muted",
				"fg-subtle",
				"fg-faint",
				"accent",
				"success",
				"info",
				"warning",
				"danger",
			],
			font: ["tight"],
		},
	},
});

export function cn(...inputs: ClassValue[]) {
	return twMerge(clsx(inputs));
}
