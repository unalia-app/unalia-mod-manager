"use client";

import { Badge, Skeleton, Spinner, cn } from "@modalia/ui";
import { ArrowUp, Download, ExternalLink, Package, RotateCw, Trash2, X } from "lucide-react";
import Link from "next/link";
import { useState } from "react";

import { Marquee } from "@/components/marquee";
import { InstallStateBadge } from "@/components/mod/install-button";
import { uninstallWithUndo } from "@/components/mod/uninstall";
import { type ModSummary, SOURCE_LABEL } from "@/lib/engine";
import { compactNumber, relativeDate } from "@/lib/format";
import { usePendingOp } from "@/lib/game-status";
import { modHref } from "@/lib/games-store";
import { cancel, install, overallProgress, useActiveInstall, useInstalled, useModUpdate } from "@/lib/installs";
import { openExternal } from "@/lib/open";

/**
 * Card of the Discover grid, in the spirit of Unalia's game cards.
 *
 * The whole card leads to the mod page (stretched link on top), except the
 * "Install" button placed above the link.
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
	 * Staggered entrance animation: for the first page only. Replayed on
	 * each page loaded on scroll, it delayed the cards and made the scroll
	 * position jump.
	 */
	animated?: boolean;
}) {
	const [image, setImage] = useState<"loading" | "loaded" | "missing">(mod.iconUrl ? "loading" : "missing");
	// Square icon (Thunderstore) in a 16:9 frame: whole, over a blurred copy
	// of itself. Nexus thumbnails, already 16:9, fill it.
	const [square, setSquare] = useState(false);
	const category = mod.categories[0];

	return (
		<article
			style={animated ? { animationDelay: `${(index % 24) * 20}ms` } : undefined}
			// No `glass` here: a backdrop-filter recomputed at each hover frame,
			// across a whole grid, stutters (especially in WebKit).
			className={cn(
				"group relative flex flex-col rounded-xl border border-line bg-surface p-2 transition-[background-color,border-color,translate] duration-300 ease-out hover:-translate-y-1 hover:border-line-strong hover:bg-surface-hover has-[:focus-visible]:border-accent/60",
				animated ? "animate-row-in" : "animate-fade-in",
			)}
		>
			<Link
				href={modHref(appId, mod.id, mod.source)}
				aria-label={mod.name}
				className="absolute inset-0 z-10 rounded-xl focus-visible:outline-none"
			/>

			{/* Container without a transform: the quick button goes above the
			    link. The image has its own layer (`transform-gpu`), without
			    which WebKit loses the rounded-corner clipping during the zoom. */}
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

				<QuickInstall appId={appId} modId={mod.id} name={mod.name} source={mod.source} />
			</div>

			<div className="flex flex-1 flex-col px-1 pt-2.5 pb-1">
				<h3 className="text-sm font-semibold text-fg">
					<Marquee text={mod.name} hoverOn="article" />
				</h3>
				<Marquee text={`by ${mod.author}`} hoverOn="article" className="text-xs text-fg-subtle" />
				<p className="mt-1.5 line-clamp-3 text-xs leading-relaxed text-fg-muted">{mod.summary}</p>

				<div className="mt-auto flex items-center gap-2 pt-3 font-tight text-[11px] text-fg-subtle">
					<span className="flex items-center gap-1" title={`${mod.downloads.toLocaleString("en-US")} downloads`}>
						<Download className="size-3" />
						{compactNumber(mod.downloads)}
					</span>
					<span aria-hidden>·</span>
					<Marquee text={relativeDate(mod.updatedAt)} hoverOn="article" />
					{/* The text is truncated inside the badge, not the badge itself:
					    same margin on the left and right. */}
					{category && (
						<Badge className="ml-auto max-w-[45%] min-w-0 shrink">
							<Marquee text={category} hoverOn="article" />
						</Badge>
					)}
				</div>
			</div>
		</article>
	);
}

/**
 * Small "Install" button on card hover: one click installs the mod with
 * its dependencies and the loader. During the install, it only shows the
 * progress; once installed, it becomes "Uninstall" ("Installed" badge at
 * the top of the card).
 */
function QuickInstall({
	appId,
	modId,
	name,
	source,
}: {
	appId: number;
	modId: string;
	name: string;
	source: string;
}) {
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
	// Free Nexus account: the download page waits for a click.
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
				void (busy
					? cancel(appId, modId)
					: website
						? openExternal(website)
						: install(appId, modId, name, undefined, source))
			}
			className={cn(
				"group/quick absolute right-2 bottom-2 z-20 flex h-7 min-w-[4.5rem] cursor-pointer items-center justify-center gap-1.5 rounded-full px-2.5 font-tight text-[11px] font-semibold shadow-lg transition-[opacity,translate,background-color] duration-200 ease-out",
				// On hover only: across a whole grid, one button per card
				// repeats too much. It stays shown during the install and on
				// failure, so the info isn't lost.
				!busy && !failed && !website &&
					"translate-y-1 opacity-0 group-hover:translate-y-0 group-hover:opacity-100 focus-visible:translate-y-0 focus-visible:opacity-100",
				// Dark glass: readable over a white or dark icon.
				failed ? "bg-danger text-bg" : "liquid-glass hover:brightness-125",
			)}
		>
			{busy ? (
				<>
					{/* In progress: the progress, and on hover a way to stop it. */}
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
