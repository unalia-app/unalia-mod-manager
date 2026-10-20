"use client";

import { Badge, Skeleton, cn } from "@modalia/ui";
import { Download, Package } from "lucide-react";
import { useState } from "react";

import type { ModSummary } from "@/lib/engine";
import { compactNumber, relativeDate } from "@/lib/format";

/** Carte de la grille Découvrir, dans l'esprit des cartes de jeu d'Unalia. */
export function ModCard({ mod, index }: { mod: ModSummary; index: number }) {
	const [image, setImage] = useState<"loading" | "loaded" | "missing">(mod.iconUrl ? "loading" : "missing");
	const category = mod.categories[0];

	return (
		<article
			style={{ animationDelay: `${(index % 24) * 20}ms` }}
			// Pas de `glass` ici : un backdrop-filter recalculé à chaque image du
			// survol, sur toute une grille, saccade (surtout dans WebKit).
			className="group flex animate-row-in flex-col rounded-xl border border-line bg-surface p-2 transition-[background-color,border-color,translate] duration-300 ease-out hover:-translate-y-1 hover:border-line-strong hover:bg-surface-hover"
		>
			{/* `transform-gpu` : sans couche dédiée, WebKit perd le découpage des
			    coins arrondis pendant le zoom de l'image. */}
			<div className="relative aspect-square transform-gpu overflow-hidden rounded-lg bg-white/[0.04]">
				{image === "loading" && <Skeleton className="absolute inset-0 rounded-none" />}
				{image === "missing" ? (
					<div className="flex size-full items-center justify-center">
						<Package className="size-8 text-fg-faint" />
					</div>
				) : (
					<img
						src={mod.iconUrl ?? ""}
						alt=""
						loading="lazy"
						decoding="async"
						onLoad={() => setImage("loaded")}
						onError={() => setImage("missing")}
						className={cn(
							"size-full object-cover transition-[opacity,scale] duration-500 ease-out group-hover:scale-[1.04]",
							image === "loaded" ? "opacity-100" : "opacity-0",
						)}
					/>
				)}
				{mod.deprecated && (
					<Badge tone="warning" className="absolute top-2 left-2 backdrop-blur-md">
						Obsolète
					</Badge>
				)}
			</div>

			<div className="flex flex-1 flex-col px-1 pt-2.5 pb-1">
				<h3 className="truncate text-sm font-semibold text-fg" title={mod.name}>
					{mod.name}
				</h3>
				<p className="truncate text-xs text-fg-subtle">par {mod.author}</p>
				<p className="mt-1.5 line-clamp-2 text-xs leading-relaxed text-fg-muted">{mod.summary}</p>

				<div className="mt-auto flex items-center gap-2 pt-3 font-tight text-[11px] text-fg-subtle">
					<span className="flex items-center gap-1" title={`${mod.downloads.toLocaleString("fr-FR")} téléchargements`}>
						<Download className="size-3" />
						{compactNumber(mod.downloads)}
					</span>
					<span aria-hidden>·</span>
					<span className="truncate">{relativeDate(mod.updatedAt)}</span>
					{category && <Badge className="ml-auto max-w-[45%] truncate">{category}</Badge>}
				</div>
			</div>
		</article>
	);
}

export function ModCardSkeleton() {
	return (
		<div className="rounded-xl border border-line bg-surface p-2">
			<Skeleton className="aspect-square rounded-lg" />
			<div className="space-y-2 px-1 pt-3 pb-1">
				<Skeleton className="h-3.5 w-3/4" />
				<Skeleton className="h-3 w-1/3" />
				<Skeleton className="h-3 w-full" />
				<Skeleton className="mt-4 h-3 w-1/2" />
			</div>
		</div>
	);
}
