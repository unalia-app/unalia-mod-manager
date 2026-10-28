"use client";

import { DiscordIcon, GitHubIcon, cn } from "@modalia/ui";

import { DISCORD_URL, REPO_URL } from "@/lib/links";
import { openExternal } from "@/lib/open";

const LINK =
	"flex cursor-pointer items-center rounded-full text-fg-faint transition-colors hover:text-fg-muted focus-visible:text-fg-muted";

/**
 * En bas à gauche de toutes les pages, discret : le code, le Discord,
 * l'auteur. L'intro (z-60) le couvre au lancement.
 */
export function Credits({ className }: { className?: string }) {
	return (
		<nav
			aria-label="Community"
			className={cn(
				"fixed bottom-4 left-4 z-20 flex items-center gap-3 rounded-full bg-bg/60 px-3 py-1.5 backdrop-blur-md",
				className,
			)}
		>
			<button type="button" className={LINK} aria-label="Source code on GitHub" onClick={() => void openExternal(REPO_URL)}>
				<GitHubIcon className="size-3.5" />
			</button>
			<button type="button" className={LINK} aria-label="Discord" onClick={() => void openExternal(DISCORD_URL)}>
				<DiscordIcon className="size-3.5" />
			</button>
			<span aria-hidden className="h-3 w-px bg-line" />
			<button type="button" className={cn(LINK, "font-tight text-[11px]")} onClick={() => void openExternal(REPO_URL)}>
				by REVERSS
			</button>
		</nav>
	);
}
