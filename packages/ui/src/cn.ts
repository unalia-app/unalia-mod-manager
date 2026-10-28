import { type ClassValue, clsx } from "clsx";
import { extendTailwindMerge } from "tailwind-merge";

// tailwind-merge must know the custom tokens so it doesn't confuse
// `text-fg-muted` (color) with `text-sm` (size).
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
