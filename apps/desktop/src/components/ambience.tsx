"use client";

import { cn } from "@modalia/ui";
import { useEffect, useState } from "react";

import { artworkUrl } from "@/lib/engine";

/**
 * Ambiance du jeu actif (SPEC §3.2) : sa bannière floutée en tête d'écran et
 * un halo teinté par l'accent. Les changements de jeu se font en fondu.
 */
export function Ambience({ appId }: { appId: number | null }) {
	// On garde les dernières bannières montées pour que l'ancienne s'efface
	// pendant que la nouvelle apparaît.
	const [stack, setStack] = useState<number[]>([]);

	useEffect(() => {
		if (appId !== null) setStack((s) => [...s.filter((id) => id !== appId), appId].slice(-3));
	}, [appId]);

	return (
		<div aria-hidden className="pointer-events-none absolute inset-0 overflow-hidden">
			<div className="absolute inset-x-0 top-0 h-[75vh] [mask-image:linear-gradient(to_bottom,black_10%,transparent)]">
				{stack.map((id) => (
					<Backdrop key={id} appId={id} visible={id === appId} />
				))}
			</div>
			<div className="absolute -top-1/3 left-1/2 h-[80vh] w-[120vw] -translate-x-1/2 bg-[radial-gradient(closest-side,color-mix(in_oklch,var(--color-accent)_14%,transparent),transparent)]" />
			<div className="absolute inset-x-0 bottom-0 h-1/2 bg-gradient-to-t from-bg to-transparent" />
		</div>
	);
}

function Backdrop({ appId, visible }: { appId: number; visible: boolean }) {
	const [loaded, setLoaded] = useState(false);
	const [failed, setFailed] = useState(false);
	if (failed) return null;

	return (
		<img
			src={artworkUrl(appId, "hero")}
			alt=""
			onLoad={() => setLoaded(true)}
			onError={() => setFailed(true)}
			className={cn(
				"absolute inset-0 size-full scale-110 object-cover blur-2xl saturate-125 transition-opacity duration-700 ease-out",
				visible && loaded ? "opacity-35" : "opacity-0",
			)}
		/>
	);
}
