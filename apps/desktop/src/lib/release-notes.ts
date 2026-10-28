/**
 * Release notes: the `CHANGELOG.md` section published with the release
 * (`notes` of `latest.json`), in the Keep a Changelog format. An intro
 * text, then `### Added`, `### Changed`, `### Fixed`… sections made of
 * bullets.
 */

export type ChangeKind = "new" | "improved" | "fixed" | "removed";

export type ReleaseNotes = {
	/** Paragraphs before the first section. */
	intro: string[];
	changes: { kind: ChangeKind; text: string }[];
};

const KINDS: Record<string, ChangeKind> = {
	added: "new",
	changed: "improved",
	deprecated: "improved",
	fixed: "fixed",
	security: "fixed",
	removed: "removed",
};

export function parseReleaseNotes(markdown: string): ReleaseNotes {
	const notes: ReleaseNotes = { intro: [], changes: [] };
	let kind: ChangeKind | null = null;
	for (const raw of markdown.split(/\r?\n/)) {
		const line = raw.trim();
		if (!line || line.startsWith("## ")) continue;
		const heading = /^###\s+(.+)$/.exec(line);
		if (heading) {
			kind = KINDS[heading[1].trim().toLowerCase()] ?? "improved";
			continue;
		}
		const bullet = /^[-*]\s+(.+)$/.exec(line);
		if (bullet) {
			notes.changes.push({ kind: kind ?? "improved", text: bullet[1] });
			continue;
		}
		const last = notes.changes.at(-1);
		// Bullet over several lines: the rest is indented.
		if (last && raw.startsWith(" ")) last.text += ` ${line}`;
		else if (kind === null) notes.intro.push(line);
	}
	return notes;
}
