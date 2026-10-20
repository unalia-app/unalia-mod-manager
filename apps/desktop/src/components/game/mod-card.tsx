"use client";

import { Badge, Skeleton, cn } from "@modalia/ui";
import { ArrowUp, Check, Download, Package, RotateCw } from "lucide-react";
import Link from "next/link";
import { useState } from "react";

import { InstallStateBadge } from "@/components/mod/install-button";
import type { ModSummary } from "@/lib/engine";
import { compactNumber, relativeDate } from "@/lib/format";
import { usePendingOp } from "@/lib/game-status";
import { modHref } from "@/lib/games-store";
import { install, overallProgress, useActiveInstall, useInstalled, useModUpdate } from "@/lib/installs";

/**
 * Carte de la grille Découvrir, dans l'esprit des cartes de jeu d'Unalia.
 *
 * Toute la carte mène à la page du mod (lien étiré par-dessus), sauf le
 * bouton d'installation rapide posé au-dessus du lien. En mode sélection,
 * un clic coche la carte au lieu de l'ouvrir.
 */
export function ModCard({
	mod,
	index,
	appId,
	selecting = false,
	selected = false,
	onToggleSelect,
}: {
	mod: ModSummary;
	index: number;
	appId: number;
	selecting?: boolean;
	selected?: boolean;
	onToggleSelect?: () => void;
}) {
	const [image, setImage] = useState<"loading" | "loaded" | "missing">(mod.iconUrl ? "loading" : "missing");
	const category = mod.categories[0];

	return (
		<article
			style={{ animationDelay: `${(index % 24) * 20}ms` }}
			// Pas de `glass` ici : un backdrop-filter recalculé à chaque image du
			// survol, sur toute une grille, saccade (surtout dans WebKit).
			className={cn(
				"group relative flex animate-row-in flex-col rounded-xl border bg-surface p-2 transition-[background-color,border-color,translate] duration-300 ease-out hover:-translate-y-1 hover:bg-surface-hover has-[:focus-visible]:border-accent/60",
				selected ? "border-accent/70 bg-accent/[0.06]" : "border-line hover:border-line-strong",
			)}
		>
			{selecting ? (
				<button
					type="button"
					aria-pressed={selected}
					aria-label={`Select ${mod.name}`}
					onClick={onToggleSelect}
					className="absolute inset-0 z-10 cursor-pointer rounded-xl focus-visible:outline-none"
				/>
			) : (
				<Link
					href={modHref(appId, mod.id)}
					aria-label={mod.name}
					className="absolute inset-0 z-10 rounded-xl focus-visible:outline-none"
				/>
			)}

			{/* Conteneur sans transform : le bouton rapide passe au-dessus du
			    lien. L'image a sa propre couche (`transform-gpu`), sans quoi
			    WebKit perd le découpage des coins arrondis pendant le zoom. */}
			<div className="relative">
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
					<InstallStateBadge appId={appId} modId={mod.id} className="absolute top-2 right-2" />
					{mod.deprecated && (
						<Badge tone="warning" className="absolute top-2 left-2 backdrop-blur-md">
							Deprecated
						</Badge>
					)}
				</div>

				{selecting ? (
					<span
						aria-hidden
						className={cn(
							"pointer-events-none absolute top-2 left-2 flex size-5 items-center justify-center rounded-md border transition-colors duration-150",
							selected ? "border-accent bg-accent text-bg" : "border-line-strong bg-black/50 backdrop-blur-md",
						)}
					>
						{selected && <Check className="size-3.5" strokeWidth={3} />}
					</span>
				) : (
					<QuickInstall appId={appId} modId={mod.id} name={mod.name} />
				)}
			</div>

			<div className="flex flex-1 flex-col px-1 pt-2.5 pb-1">
				<h3 className="truncate text-sm font-semibold text-fg" title={mod.name}>
					{mod.name}
				</h3>
				<p className="truncate text-xs text-fg-subtle">by {mod.author}</p>
				<p className="mt-1.5 line-clamp-2 text-xs leading-relaxed text-fg-muted">{mod.summary}</p>

				<div className="mt-auto flex items-center gap-2 pt-3 font-tight text-[11px] text-fg-subtle">
					<span className="flex items-center gap-1" title={`${mod.downloads.toLocaleString("en-US")} downloads`}>
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

/**
 * Installation en un clic depuis la carte (SPEC §3.4) : apparaît au survol,
 * devient un anneau de progression pendant l'installation.
 */
function QuickInstall({ appId, modId, name }: { appId: number; modId: string; name: string }) {
	const installed = useInstalled(appId)?.some((m) => m.id === modId);
	const update = useModUpdate(appId, modId);
	const active = useActiveInstall(appId, modId);
	const pending = usePendingOp(appId, modId);

	if (pending) return null;

	if (active?.status === "queued" || active?.status === "running") {
		const value = active.status === "running" ? overallProgress(active.progress) : 0;
		return (
			<span
				role="progressbar"
				aria-label={`Installing ${name}`}
				aria-valuenow={Math.round(value * 100)}
				className="absolute right-2 bottom-2 z-20 flex size-9 items-center justify-center rounded-full bg-black/60 backdrop-blur-md"
			>
				<ProgressRing value={value} />
			</span>
		);
	}

	if (installed && !update) return null;

	const failed = active?.status === "error";
	const Icon = failed ? RotateCw : update ? ArrowUp : Download;
	const label = failed ? `Try installing ${name} again` : update ? `Update ${name}` : `Install ${name}`;

	return (
		<button
			type="button"
			aria-label={label}
			title={failed ? active.message : label}
			onClick={() => void install(appId, modId)}
			className={cn(
				"absolute right-2 bottom-2 z-20 flex size-9 cursor-pointer items-center justify-center rounded-full shadow-lg transition-[opacity,translate,background-color] duration-200 ease-out",
				"translate-y-1 opacity-0 group-hover:translate-y-0 group-hover:opacity-100 focus-visible:translate-y-0 focus-visible:opacity-100 focus-visible:outline-none",
				failed ? "bg-danger text-bg opacity-100" : "bg-white text-bg hover:bg-white/85",
			)}
		>
			<Icon className="size-4" strokeWidth={2.5} />
		</button>
	);
}

function ProgressRing({ value }: { value: number }) {
	const r = 13;
	const circumference = 2 * Math.PI * r;
	return (
		<svg viewBox="0 0 32 32" className="size-7 -rotate-90" aria-hidden>
			<circle cx="16" cy="16" r={r} fill="none" strokeWidth="3" className="stroke-white/15" />
			<circle
				cx="16"
				cy="16"
				r={r}
				fill="none"
				strokeWidth="3"
				strokeLinecap="round"
				className="stroke-accent transition-[stroke-dashoffset] duration-300 ease-out"
				strokeDasharray={circumference}
				strokeDashoffset={circumference * (1 - Math.max(value, 0.04))}
			/>
		</svg>
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
