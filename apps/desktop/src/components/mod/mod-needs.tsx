"use client";

import { cn } from "@modalia/ui";
import { ExternalLink, X } from "lucide-react";
import { useRouter } from "next/navigation";
import { useState } from "react";

import type { InstalledMod, NexusRequirement } from "@/lib/engine";
import { modHref } from "@/lib/games-store";
import { openExternal } from "@/lib/open";

const hiddenKey = (m: InstalledMod) => `modalia.needsHidden.${m.game}.${m.id}`;

const signature = (m: InstalledMod) => m.needs.map((r) => r.name).join("|");

function isHidden(m: InstalledMod): boolean {
	try {
		return localStorage.getItem(hiddenKey(m)) === signature(m);
	} catch {
		return false;
	}
}

/**
 * What a mod still needs and Unalia can't install by itself: always in
 * view, one click from each page, gone once installed. Not an error: the
 * mod is installed. Hidden by hand for this list only: Unalia can't see a
 * requirement from another site (ReShade) arrive.
 */
export function ModNeeds({ mod: m, appId, className }: { mod: InstalledMod; appId: number; className?: string }) {
	const router = useRouter();
	const [hidden, setHidden] = useState(() => isHidden(m));

	if (m.needs.length === 0 || hidden) return null;

	const open = (requirement: NexusRequirement) => {
		if (requirement.modId !== null) router.push(modHref(appId, `nexus-${requirement.modId}`));
		else if (requirement.url) void openExternal(requirement.url);
	};

	const hide = () => {
		setHidden(true);
		try {
			localStorage.setItem(hiddenKey(m), signature(m));
		} catch {
			// Storage blocked: it comes back next time, no big deal.
		}
	};

	return (
		<div className={cn("flex flex-wrap items-center gap-1.5", className)}>
			<span className="text-[11px] text-fg-subtle">Also needs</span>
			{m.needs.map((requirement) => (
				<button
					key={requirement.name}
					type="button"
					title={requirement.notes || undefined}
					onClick={() => open(requirement)}
					className="flex max-w-full cursor-pointer items-center gap-1 rounded-md border border-white/10 bg-white/4 px-2 py-0.5 text-[11px] font-medium text-fg-muted transition-colors hover:border-white/18 hover:text-fg"
				>
					<span className="truncate">{requirement.name}</span>
					{requirement.modId === null && <ExternalLink className="size-3 shrink-0" />}
				</button>
			))}
			<button
				type="button"
				aria-label={`Hide what ${m.name} needs`}
				title="Hide"
				onClick={hide}
				className="cursor-pointer rounded-md p-1 text-fg-faint transition-colors hover:bg-white/10 hover:text-fg"
			>
				<X className="size-3" />
			</button>
		</div>
	);
}
