"use client";

import { Badge, Button, Skeleton, cn } from "@modalia/ui";
import { ArrowLeft, Download, ExternalLink, Globe, Heart, Package, RotateCw, TriangleAlert } from "lucide-react";
import Link from "next/link";
import { useRouter, useSearchParams } from "next/navigation";
import { type CSSProperties, type ReactNode, useEffect, useMemo, useState } from "react";

import { Ambience } from "@/components/ambience";
import { AppHeader } from "@/components/app-header";
import { GameSwitcher } from "@/components/game/game-switcher";
import { PlayButton } from "@/components/game/play-button";
import { Markdown } from "@/components/markdown";
import { GalleryStrip, ImageViewer, descriptionImages, useGallery } from "@/components/mod/gallery";
import { InstallButton } from "@/components/mod/install-button";
import { Notice } from "@/components/notice";
import { useAccent } from "@/hooks/use-accent";
import {
	type DependencyInfo,
	type ModDetails,
	SOURCE_LABEL,
	type SteamGame,
	getMod,
	modDescription,
	sourceName,
} from "@/lib/engine";
import { compactNumber, relativeDate } from "@/lib/format";
import { findGame, gameHref, modHref } from "@/lib/games-store";
import { useNexusAccount } from "@/lib/nexus";
import { openExternal } from "@/lib/open";

type Load<T> = { status: "loading" } | { status: "ready"; value: T } | { status: "error"; message: string };

/** Detail page of a mod. */
export function ModScreen() {
	const params = useSearchParams();
	const appId = Number(params.get("game"));
	const modId = params.get("id") ?? "";
	// Opened from Hexium's Discover: the mod is looked up on Hexium first.
	const hexium = params.get("source") === "hexium";

	const [game, setGame] = useState<SteamGame | null>(null);
	const [details, setDetails] = useState<Load<ModDetails>>({ status: "loading" });
	const [description, setDescription] = useState<Load<string>>({ status: "loading" });
	const [attempt, setAttempt] = useState(0);
	const accent = useAccent(game?.appId ?? null);

	useEffect(() => {
		let current = true;
		setDetails({ status: "loading" });
		setDescription({ status: "loading" });
		findGame(appId)
			.then((found) => {
				if (!current) return;
				setGame(found);
				if (!found?.profile) throw new Error("This game isn't supported.");
				const community = hexium ? `hexium:${found.profile.community}` : found.profile.community;
				return getMod(community, modId).then((mod) => ({ mod, community }));
			})
			.then((loaded) => {
				if (!current || !loaded) return;
				const { mod, community } = loaded;
				setDetails({ status: "ready", value: mod });
				const version = mod.versions[0]?.number;
				if (!version) return setDescription({ status: "ready", value: "" });
				return modDescription(community, mod.id, version).then(
					(markdown) => current && setDescription({ status: "ready", value: markdown }),
					(e: unknown) => current && setDescription({ status: "error", message: String(e) }),
				);
			})
			.catch((e: unknown) => current && setDetails({ status: "error", message: String(e) }));
		return () => {
			current = false;
		};
	}, [appId, modId, hexium, attempt]);

	const retry = () => setAttempt((a) => a + 1);

	return (
		<main
			className="accent-transition relative min-h-dvh overflow-x-clip"
			style={accent ? ({ "--accent": accent } as CSSProperties) : undefined}
		>
			<Ambience appId={game?.appId ?? null} />
			<AppHeader actions={game?.moddable && <PlayButton appId={game.appId} compact />}>
				{game && <GameSwitcher current={game} />}
			</AppHeader>

			<div className="relative z-10 mx-auto w-full max-w-[120rem] px-8 pb-20 xl:px-12">
				<Link
					href={gameHref(appId)}
					className="mt-6 inline-flex items-center gap-1.5 text-xs font-medium text-fg-subtle transition-colors hover:text-fg"
				>
					<ArrowLeft className="size-3.5" />
					{game ? `${game.name} mods` : "Back"}
				</Link>

				{details.status === "loading" && <HeroSkeleton />}

				{details.status === "error" && (
					<div className="glass mt-10 max-w-xl">
						<Notice
							icon={<TriangleAlert className="size-5 text-danger" />}
							title="Couldn't load this mod"
							action={
								<Button variant="outline" size="sm" leftIcon={<RotateCw className="size-3.5" />} onClick={retry}>
									Try again
								</Button>
							}
							details={details.message}
						>
							Check your connection, or open it later.
						</Notice>
					</div>
				)}

				{details.status === "ready" && game && (
					<ModDetailsView key={details.value.id} mod={details.value} game={game} description={description} />
				)}
			</div>
		</main>
	);
}

function ModDetailsView({
	mod,
	game,
	description,
}: {
	mod: ModDetails;
	game: SteamGame;
	description: Load<string>;
}) {
	const latest = mod.versions[0];
	const community = game.profile?.community ?? "";
	const nexus = game.profile?.nexus ?? null;
	const hexium = mod.source === "hexium";
	const source = SOURCE_LABEL[mod.source] ?? sourceName(game);
	// Nexus doesn't publish a stable author page by name: plain text.
	const authorUrl = nexus
		? null
		: hexium
			? `https://${community}.hexium.gg/teams/${mod.author}`
			: `https://thunderstore.io/c/${community}/p/${mod.author}/`;
	const extraDependencies = mod.dependencies.filter((d) => !d.loader).length;
	const account = useNexusAccount();
	const hint = !nexus
		? undefined
		: account === null && !mod.direct
			? "Link your Nexus Mods account in Settings first"
			: account && !account.premium && !mod.direct
				? "Nexus asks free accounts to confirm each download on its website"
				: undefined;

	// The main picture, then the screenshots of the description.
	const markdown = description.status === "ready" ? description.value : "";
	const candidates = useMemo(
		() => [...new Set([...mod.images.slice(0, 1), ...descriptionImages(markdown)])],
		[mod.images, markdown],
	);
	const { images, measured } = useGallery(candidates);
	// The header's picture opens too, even when it is too small for the row
	// (a Thunderstore icon).
	const main = mod.images[0];
	const viewable = useMemo(
		() => (main && !images.includes(main) ? [main, ...images] : images),
		[main, images],
	);
	const [viewing, setViewing] = useState<number | null>(null);
	const zoom = useMemo(
		() => ({
			can: (src: string) => viewable.includes(src),
			open: (src: string) => setViewing(viewable.indexOf(src)),
		}),
		[viewable],
	);

	return (
		<>
			<section className="mt-6 flex items-start gap-6">
				<ModIcon
					src={main}
					onOpen={main ? () => zoom.open(main) : undefined}
					className="size-28 animate-fade-up rounded-xl shadow-2xl shadow-black/50"
				/>
				<div className="min-w-0 flex-1 animate-fade-up [animation-delay:60ms]">
					<h1 className="truncate pb-1 text-4xl leading-tight font-semibold tracking-tight">{mod.name}</h1>
					<p className="text-sm text-fg-subtle">
						by{" "}
						{authorUrl ? (
							<ExternalTextLink href={authorUrl} className="text-fg-muted hover:text-fg">
								{mod.author}
							</ExternalTextLink>
						) : (
							<span className="text-fg-muted">{mod.author}</span>
						)}
						{mod.donationUrl && (
							<>
								{" · "}
								<ExternalTextLink href={mod.donationUrl} className="inline-flex items-center gap-1 text-accent">
									<Heart className="size-3 fill-current" />
									Support
								</ExternalTextLink>
							</>
						)}
					</p>
					<p className="mt-3 max-w-2xl text-sm leading-relaxed text-fg-muted">{mod.summary}</p>

					<div className="mt-4 flex flex-wrap items-center gap-2">
						{mod.downloads !== null && (
							<Badge>
								<Download />
								{compactNumber(mod.downloads)} downloads
							</Badge>
						)}
						{latest && <Badge>v{latest.number}</Badge>}
						<Badge>Updated {relativeDate(mod.updatedAt)}</Badge>
						{mod.categories.map((c) => (
							<Badge key={c} tone="accent">
								{c}
							</Badge>
						))}
					</div>
				</div>

				<div className="shrink-0 animate-fade-up [animation-delay:120ms]">
					<InstallButton
						appId={game.appId}
						modId={mod.id}
						modName={mod.name}
						dependencyCount={extraDependencies}
						hint={hint}
						source={mod.source}
					/>
				</div>
			</section>

			<GalleryStrip
				candidates={candidates}
				images={images}
				measured={measured}
				onOpen={(index) => setViewing(viewable.indexOf(images[index] ?? ""))}
			/>
			{viewing !== null && (
				<ImageViewer images={viewable} index={viewing} onIndex={setViewing} onClose={() => setViewing(null)} />
			)}

			{mod.deprecated && (
				<div className="mt-6 max-w-2xl rounded-xl border border-warning/20 bg-warning/10 px-4 py-3 text-xs text-warning">
					Its author marked this mod as deprecated. It may not work with the current version of the game.
				</div>
			)}

			<div className="mt-10 grid animate-fade-up gap-6 [animation-delay:160ms] lg:grid-cols-[minmax(0,1fr)_18rem]">
				<article className="rounded-xl border border-line bg-surface p-6">
					{description.status === "loading" && <DescriptionSkeleton />}
					{description.status === "error" && (
						<Notice
							icon={<TriangleAlert className="size-5 text-danger" />}
							title="Couldn't load the description"
							details={description.message}
						>
							The rest of the page is up to date. You can read it on {source}.
						</Notice>
					)}
					{description.status === "ready" &&
						(description.value.trim() ? (
							<Markdown internalLink={(href) => modLink(href, game)} zoom={zoom}>
								{description.value}
							</Markdown>
						) : (
							<p className="text-sm text-fg-subtle">The author didn't write a description.</p>
						))}
				</article>

				<aside className="flex flex-col gap-4 self-start lg:sticky lg:top-20">
					<Panel title={`Dependencies (${mod.dependencies.length})`}>
						{mod.dependencies.length === 0 ? (
							<p className="px-1 text-xs text-fg-subtle">None. This mod works on its own.</p>
						) : (
							<ul className="-mx-1">
								{mod.dependencies.map((d) => (
									<li key={d.id}>
										<DependencyRow
											dependency={d}
											appId={game.appId}
											nexus={Boolean(nexus)}
											source={mod.source}
										/>
									</li>
								))}
							</ul>
						)}
					</Panel>

					<Panel title="Links">
						{mod.donationUrl && (
							<button
								type="button"
								onClick={() => void openExternal(mod.donationUrl ?? "")}
								className="mb-3 flex w-full cursor-pointer items-center gap-3 rounded-lg border border-accent/25 bg-accent/[0.08] px-3 py-2.5 text-left transition-colors hover:bg-accent/[0.14]"
							>
								<Heart className="size-4 shrink-0 fill-accent text-accent" />
								<span className="min-w-0">
									<span className="block text-sm font-medium text-fg">Support the author</span>
									<span className="block text-[11px] text-fg-subtle">Your whole donation goes to {mod.author}</span>
								</span>
							</button>
						)}
						<div className="flex flex-col items-start gap-1">
							<ExternalButton href={mod.sourceUrl} icon={<ExternalLink className="size-3.5" />}>
								View on {source}
							</ExternalButton>
							{mod.websiteUrl && (
								<ExternalButton href={mod.websiteUrl} icon={<Globe className="size-3.5" />}>
									{websiteLabel(mod.websiteUrl)}
								</ExternalButton>
							)}
						</div>
					</Panel>
				</aside>
			</div>
		</>
	);
}

function DependencyRow({
	dependency: d,
	appId,
	nexus,
	source,
}: {
	dependency: DependencyInfo;
	appId: number;
	nexus: boolean;
	/** Source of the mod that needs it: a Hexium mod's dependency is looked up on Hexium first. */
	source: string;
}) {
	const router = useRouter();
	// Thunderstore publishes each version's icon at a predictable URL.
	const icon = nexus ? undefined : `https://gcdn.thunderstore.io/live/repository/icons/${d.id}-${d.minVersion}.png`;
	const detail = nexus
		? d.automatic
			? "Installed automatically"
			: "Install it too, from Nexus Mods"
		: `by ${d.author} · ${d.minVersion} or newer`;

	const content = (
		<>
			<ModIcon src={icon} className="size-9 rounded-md" />
			<span className="min-w-0 flex-1">
				<span className="block truncate text-sm font-medium text-fg">{d.name}</span>
				<span className="block truncate text-[11px] text-fg-subtle">{detail}</span>
			</span>
		</>
	);

	if (d.loader) {
		return (
			<div
				className="flex items-center gap-2.5 rounded-lg px-1 py-1.5"
				title={nexus ? "Installed automatically from its author's GitHub" : "Installed automatically with your first mod"}
			>
				{content}
				<Badge tone="accent">{nexus ? "Framework" : "Loader"}</Badge>
			</div>
		);
	}
	return (
		<button
			type="button"
			onClick={() => router.push(modHref(appId, d.id, source))}
			className="flex w-full cursor-pointer items-center gap-2.5 rounded-lg px-1 py-1.5 text-left transition-colors hover:bg-surface-hover"
		>
			{content}
		</button>
	);
}

/** `onOpen`: the picture opens in the viewer once it has loaded. */
function ModIcon({ src, className, onOpen }: { src?: string; className?: string; onOpen?: () => void }) {
	const [status, setStatus] = useState<"loading" | "loaded" | "missing">(src ? "loading" : "missing");
	const open = onOpen && status === "loaded" ? onOpen : undefined;
	return (
		<span
			role={open ? "button" : undefined}
			tabIndex={open ? 0 : undefined}
			aria-label={open ? "View the picture" : undefined}
			onClick={open}
			onKeyDown={open ? (e) => (e.key === "Enter" || e.key === " ") && open() : undefined}
			className={cn(
				"relative flex shrink-0 items-center justify-center overflow-hidden border border-line bg-white/[0.04]",
				open && "cursor-zoom-in transition hover:brightness-110",
				className,
			)}
		>
			{status !== "loaded" && <Package className="size-1/3 text-fg-faint" />}
			{src && status !== "missing" && (
				<img
					src={src}
					alt=""
					onLoad={() => setStatus("loaded")}
					onError={() => setStatus("missing")}
					className={cn(
						"absolute inset-0 size-full object-cover transition-opacity duration-300",
						status === "loaded" ? "opacity-100" : "opacity-0",
					)}
				/>
			)}
		</span>
	);
}

function Panel({ title, children }: { title: string; children: ReactNode }) {
	return (
		<section className="rounded-xl border border-line bg-surface p-4">
			<h2 className="mb-3 px-1 text-xs font-semibold tracking-wide text-fg-subtle uppercase">{title}</h2>
			{children}
		</section>
	);
}

function ExternalButton({ href, icon, children }: { href: string; icon: ReactNode; children: ReactNode }) {
	return (
		<Button variant="secondary" size="sm" leftIcon={icon} className="-ml-2" onClick={() => void openExternal(href)}>
			{children}
		</Button>
	);
}

function ExternalTextLink({ href, className, children }: { href: string; className?: string; children: ReactNode }) {
	return (
		<button
			type="button"
			onClick={() => void openExternal(href)}
			className={cn("cursor-pointer underline-offset-2 transition-colors hover:underline", className)}
		>
			{children}
		</button>
	);
}

/**
 * Link to another mod of the same game, on its source: its page in
 * Unalia. The download follows the source's rules (Nexus: its site).
 */
function modLink(href: string, game: SteamGame): string | null {
	const profile = game.profile;
	if (!profile) return null;
	let url: URL;
	try {
		url = new URL(href);
	} catch {
		return null;
	}
	const host = url.hostname.replace(/^www\./, "");
	const parts = url.pathname.split("/").filter(Boolean);
	if (profile.nexus && host === "nexusmods.com") {
		const [domain, kind, id] = parts;
		if (domain === profile.nexus.domain && kind === "mods" && id && /^\d+$/.test(id)) {
			return modHref(game.appId, `nexus-${id}`);
		}
	}
	if (!profile.nexus && host === "thunderstore.io") {
		// /c/<community>/p/<namespace>/<name>/
		const [c, community, p, namespace, name] = parts;
		if (c === "c" && community === profile.community && p === "p" && namespace && name) {
			return modHref(game.appId, `${namespace}-${name}`);
		}
	}
	return null;
}

/** `https://github.com/MCXGK3/KnightInSilkSong` → `GitHub`. */
function websiteLabel(url: string): string {
	try {
		const host = new URL(url).hostname.replace(/^www\./, "");
		return host === "github.com" ? "Source code on GitHub" : host;
	} catch {
		return "Website";
	}
}

function HeroSkeleton() {
	return (
		<div className="mt-6 flex items-start gap-6">
			<Skeleton className="size-28 rounded-xl" />
			<div className="flex-1 space-y-3 pt-1">
				<Skeleton className="h-9 w-2/5" />
				<Skeleton className="h-3.5 w-24" />
				<Skeleton className="h-3.5 w-3/5" />
				<div className="flex gap-2 pt-2">
					<Skeleton className="h-5 w-24 rounded-full" />
					<Skeleton className="h-5 w-14 rounded-full" />
				</div>
			</div>
		</div>
	);
}

function DescriptionSkeleton() {
	return (
		<div className="space-y-3">
			<Skeleton className="h-5 w-1/3" />
			{[100, 92, 96, 70].map((w) => (
				<Skeleton key={w} className="h-3.5" style={{ width: `${w}%` }} />
			))}
			<Skeleton className="mt-6 h-40 w-full rounded-lg" />
		</div>
	);
}
