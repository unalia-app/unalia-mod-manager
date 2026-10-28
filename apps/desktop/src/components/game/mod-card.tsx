"use client";

import { Badge, Skeleton, Spinner, cn } from "@modalia/ui";
import { ArrowUp, Download, ExternalLink, Package, RotateCw, Trash2, X } from "lucide-react";
import Link from "next/link";
import { useState } from "react";

import { InstallStateBadge } from "@/components/mod/install-button";
import { uninstallWithUndo } from "@/components/mod/uninstall";
import type { ModSummary } from "@/lib/engine";
import { compactNumber, relativeDate } from "@/lib/format";
import { usePendingOp } from "@/lib/game-status";
import { modHref } from "@/lib/games-store";
import { cancel, install, overallProgress, useActiveInstall, useInstalled, useModUpdate } from "@/lib/installs";
import { openExternal } from "@/lib/open";

/** Nom affiché de la source d'un mod (`ModSummary.source`). */
const SOURCE_LABEL: Record<string, string> = {
	nexus: "Nexus Mods",
	thunderstore: "Thunderstore",
};

/**
 * Carte de la grille Découvrir, dans l'esprit des cartes de jeu d'Unalia.
 *
 * Toute la carte mène à la page du mod (lien étiré par-dessus), sauf le
 * bouton « Install » posé au-dessus du lien.
 */
export function ModCard({
	mod,
	index,
	appId,
	animated = true,
}: {
	mod: ModSummary;
	index: number;
	appId: number;
	/**
	 * Entrée animée en cascade : pour la première page seulement. Rejouée
	 * à chaque page chargée au défilement, elle retardait l'arrivée des
	 * cartes et faisait sauter la position de défilement.
	 */
	animated?: boolean;
}) {
	const [image, setImage] = useState<"loading" | "loaded" | "missing">(mod.iconUrl ? "loading" : "missing");
	// Icône carrée (Thunderstore) dans un cadre 16:9 : entière, sur un fond
	// flou d'elle-même. Les vignettes de Nexus, déjà en 16:9, le remplissent.
	const [square, setSquare] = useState(false);
	const category = mod.categories[0];

	return (
		<article
			style={animated ? { animationDelay: `${(index % 24) * 20}ms` } : undefined}
			// Pas de `glass` ici : un backdrop-filter recalculé à chaque image du
			// survol, sur toute une grille, saccade (surtout dans WebKit).
			className={cn(
				"group relative flex flex-col rounded-xl border border-line bg-surface p-2 transition-[background-color,border-color,translate] duration-300 ease-out hover:-translate-y-1 hover:border-line-strong hover:bg-surface-hover has-[:focus-visible]:border-accent/60",
				animated ? "animate-row-in" : "animate-fade-in",
			)}
		>
			<Link
				href={modHref(appId, mod.id)}
				aria-label={mod.name}
				className="absolute inset-0 z-10 rounded-xl focus-visible:outline-none"
			/>

			{/* Conteneur sans transform : le bouton rapide passe au-dessus du
			    lien. L'image a sa propre couche (`transform-gpu`), sans quoi
			    WebKit perd le découpage des coins arrondis pendant le zoom. */}
			<div className="relative">
				<div className="relative aspect-video transform-gpu overflow-hidden rounded-lg bg-white/[0.04]">
					{image === "loading" && <Skeleton className="absolute inset-0 rounded-none" />}
					{image === "missing" ? (
						<div className="flex size-full items-center justify-center">
							<Package className="size-8 text-fg-faint" />
						</div>
					) : (
						<>
							{square && (
								<img
									src={mod.iconUrl ?? ""}
									alt=""
									aria-hidden
									className="absolute inset-0 size-full scale-125 object-cover opacity-50 blur-xl"
								/>
							)}
							<img
								src={mod.iconUrl ?? ""}
								alt=""
								loading="lazy"
								decoding="async"
								onLoad={(e) => {
									const { naturalWidth, naturalHeight } = e.currentTarget;
									setSquare(naturalHeight > 0 && naturalWidth / naturalHeight < 1.4);
									setImage("loaded");
								}}
								onError={() => setImage("missing")}
								className={cn(
									"relative size-full transition-[opacity,scale] duration-500 ease-out group-hover:scale-[1.04]",
									square ? "object-contain" : "object-cover",
									image === "loaded" ? "opacity-100" : "opacity-0",
								)}
							/>
						</>
					)}
					<InstallStateBadge appId={appId} modId={mod.id} className="absolute top-2 right-2" />
					<span className="liquid-glass absolute bottom-2 left-2 rounded-full px-2 py-0.5 font-tight text-[10px] font-semibold">
						{SOURCE_LABEL[mod.source] ?? mod.source}
					</span>
					{mod.deprecated && (
						<Badge tone="warning" className="absolute top-2 left-2 backdrop-blur-md">
							Deprecated
						</Badge>
					)}
				</div>

				<QuickInstall appId={appId} modId={mod.id} name={mod.name} />
			</div>

			<div className="flex flex-1 flex-col px-1 pt-2.5 pb-1">
				<h3 className="truncate text-sm font-semibold text-fg" title={mod.name}>
					{mod.name}
				</h3>
				<p className="truncate text-xs text-fg-subtle">by {mod.author}</p>
				<p className="mt-1.5 line-clamp-3 text-xs leading-relaxed text-fg-muted">{mod.summary}</p>

				<div className="mt-auto flex items-center gap-2 pt-3 font-tight text-[11px] text-fg-subtle">
					<span className="flex items-center gap-1" title={`${mod.downloads.toLocaleString("en-US")} downloads`}>
						<Download className="size-3" />
						{compactNumber(mod.downloads)}
					</span>
					<span aria-hidden>·</span>
					<span className="truncate">{relativeDate(mod.updatedAt)}</span>
					{/* Le texte est tronqué dans le badge, pas le badge lui-même : même
					    marge à gauche et à droite. */}
					{category && (
						<Badge className="ml-auto max-w-[45%] min-w-0 shrink" title={category}>
							<span className="min-w-0 truncate">{category}</span>
						</Badge>
					)}
				</div>
			</div>
		</article>
	);
}

/**
 * Petit bouton « Install » au survol de la carte (SPEC §3.4) : un
 * clic installe le mod avec ses dépendances et le loader. Pendant
 * l'installation, il montre juste l'avancement ; une fois installé, il
 * devient « Uninstall » (badge « Installed » en haut de la carte).
 */
function QuickInstall({ appId, modId, name }: { appId: number; modId: string; name: string }) {
	const installed = useInstalled(appId)?.some((m) => m.id === modId);
	const update = useModUpdate(appId, modId);
	const active = useActiveInstall(appId, modId);
	const pending = usePendingOp(appId, modId);
	const [removing, setRemoving] = useState(false);

	if (pending) return null;
	const busy = active?.status === "queued" || active?.status === "running";
	if (installed && !update && !busy) {
		return (
			<button
				type="button"
				aria-label={`Uninstall ${name}`}
				disabled={removing}
				onClick={async () => {
					setRemoving(true);
					await uninstallWithUndo(appId, modId, name);
					setRemoving(false);
				}}
				className={cn(
					"liquid-glass absolute right-2 bottom-2 z-20 flex h-7 min-w-[4.5rem] cursor-pointer items-center justify-center gap-1.5 rounded-full px-2.5 font-tight text-[11px] font-semibold shadow-lg transition-[opacity,translate,color] duration-200 ease-out hover:text-danger",
					!removing &&
						"translate-y-1 opacity-0 group-hover:translate-y-0 group-hover:opacity-100 focus-visible:translate-y-0 focus-visible:opacity-100",
				)}
			>
				{removing ? <Spinner className="size-3 border-[1.5px]" /> : <Trash2 className="size-3" strokeWidth={2.5} />}
				Uninstall
			</button>
		);
	}

	const failed = active?.status === "error";
	// Compte Nexus gratuit : la page de téléchargement attend un clic.
	const website = active?.status === "website" ? active.url : null;
	const percent = active?.status === "running" ? Math.round(overallProgress(active.progress) * 100) : null;
	const Icon = failed ? RotateCw : website ? ExternalLink : update ? ArrowUp : Download;
	const label = failed ? "Retry" : website ? "On Nexus" : update ? "Update" : "Install";

	return (
		<button
			type="button"
			aria-label={busy ? `Cancel installing ${name}` : `${label} ${name}`}
			title={failed ? active.message : website ? "Click Slow download on Nexus Mods, Unalia does the rest" : undefined}
			onClick={() =>
				void (busy ? cancel(appId, modId) : website ? openExternal(website) : install(appId, modId, name))
			}
			className={cn(
				"group/quick absolute right-2 bottom-2 z-20 flex h-7 min-w-[4.5rem] cursor-pointer items-center justify-center gap-1.5 rounded-full px-2.5 font-tight text-[11px] font-semibold shadow-lg transition-[opacity,translate,background-color] duration-200 ease-out",
				// Au survol seulement : sur toute une grille, un bouton par carte
				// se répète trop. Il reste affiché pendant l'installation et en
				// cas d'échec, pour ne pas perdre l'info.
				!busy && !failed && !website &&
					"translate-y-1 opacity-0 group-hover:translate-y-0 group-hover:opacity-100 focus-visible:translate-y-0 focus-visible:opacity-100",
				// Verre sombre : lisible sur une icône blanche comme sombre.
				failed ? "bg-danger text-bg" : "liquid-glass hover:brightness-125",
			)}
		>
			{busy ? (
				<>
					{/* En cours : l'avancement, et au survol de quoi l'arrêter. */}
					<span className="flex items-center gap-1.5 group-hover/quick:hidden group-focus-visible/quick:hidden">
						<Spinner className="size-3 border-[1.5px]" />
						{percent !== null ? `${percent}%` : "…"}
					</span>
					<span className="hidden items-center gap-1 group-hover/quick:flex group-focus-visible/quick:flex">
						<X className="size-3" strokeWidth={2.5} />
						Cancel
					</span>
				</>
			) : (
				<>
					<Icon className="size-3" strokeWidth={2.5} />
					{label}
				</>
			)}
		</button>
	);
}

export function ModCardSkeleton() {
	return (
		<div className="rounded-xl border border-line bg-surface p-2">
			<Skeleton className="aspect-video rounded-lg" />
			<div className="space-y-2 px-1 pt-3 pb-1">
				<Skeleton className="h-3.5 w-3/4" />
				<Skeleton className="h-3 w-1/3" />
				<Skeleton className="h-3 w-full" />
				<Skeleton className="mt-4 h-3 w-1/2" />
			</div>
		</div>
	);
}
