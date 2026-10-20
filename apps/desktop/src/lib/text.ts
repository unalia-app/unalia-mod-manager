/** Recherche tolérante : casse, accents et ponctuation ignorés. */
export function normalize(text: string): string {
	return text
		.normalize("NFD")
		.replace(/\p{Diacritic}/gu, "")
		.toLowerCase()
		.replace(/[^\p{L}\p{N}]+/gu, " ")
		.trim();
}

export function initials(name: string): string {
	const words = normalize(name).split(" ").filter(Boolean);
	return words
		.slice(0, 2)
		.map((w) => w[0]?.toUpperCase())
		.join("");
}
