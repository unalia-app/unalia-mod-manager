"use client";

import { Badge, Button, SearchField, Skeleton, cn } from "@modalia/ui";
import { ChevronRight, FolderPlus, Gamepad2, Package, RotateCw, TriangleAlert } from "lucide-react";
import Link from "next/link";
import { useRouter } from "next/navigation";
import { type KeyboardEvent, type ReactNode, useState } from "react";

import { Notice } from "@/components/notice";
import { toast } from "@/components/toaster";
import { pickAndAddGame } from "@/lib/add-game";
import { type GameScan, type SteamGame, artworkUrl, gameSubtitle, isModdable, splitError } from "@/lib/engine";
import { gameHref, rememberGame } from "@/lib/games-store";
import { useInstalled } from "@/lib/installs";
import { initials, normalize } from "@/lib/text";

export type LibraryState =
	| { status: "loading" }
	| { status: "ready"; scan: GameScan }
	| { status: "error"; message: string };

/**
 * Bibliothèque de l'accueil, comme le backlog d'Unalia : les jeux moddables en
 * jaquettes, les autres repliés en bas. La recherche filtre les deux.
 */
export function GameLibrary({
	state,
	games,
	others,
	onRetry,
	onRefresh,
	refreshing,
	onFocusChange,
}: {
	state: LibraryState;
	/** Jeux moddables, récents en tête. */
	games: SteamGame[];
	/** Jeux que Unalia ne sait pas encore modder. */
	others: SteamGame[];
	onRetry: () => void;
	/** Relit la bibliothèque sans vider la grille. */
	onRefresh: () => void;
	refreshing: boolean;
	/** Jeu survolé, pour l'ambiance (`null` en sortant de la grille). */
	onFocusChange: (game: SteamGame | null) => void;
}) {
	const router = useRouter();
	const [query, setQuery] = useState("");
	const [adding, setAdding] = useState(false);

	const q = normalize(query);
	const matches = (g: SteamGame) => !q || normalize(g.name).includes(q);
	const shown = games.filter(matches);
	const hidden = others.filter(matches);

	const addGame = async () => {
		setAdding(true);
		try {
			const appId = await pickAndAddGame();
			if (appId !== null) router.push(gameHref(appId));
		} catch (error) {
			const { message, details } = splitError(error);
			toast({ title: message, details: details ?? undefined, tone: "error", duration: 8000 });
		} finally {
			setAdding(false);
		}
	};

	const onKeyDown = (event: KeyboardEvent<HTMLInputElement>) => {
		const first = shown[0];
		if (event.key === "Enter" && first) {
			rememberGame(first.appId);
			router.push(gameHref(first.appId));
		}
	};

	return (
		<section className="animate-fade-up [animation-delay:60ms]">
			<div className="mb-5 flex flex-wrap items-center justify-between gap-3">
				<SectionTitle icon={<Gamepad2 />} count={state.status === "ready" ? games.length : undefined}>
					Your games
				</SectionTitle>
				<div className="flex items-center gap-2">
					<SearchField
						value={query}
						onChange={(e) => setQuery(e.target.value)}
						onKeyDown={onKeyDown}
						placeholder="Search your games"
						aria-label="Search your games"
						className="h-10 w-64"
					/>
					<Button
						variant="outline"
						size="icon"
						className="size-10"
						aria-label="Refresh your games"
						title="Look again for games installed since Unalia opened"
						disabled={state.status !== "ready" || refreshing}
						onClick={onRefresh}
					>
						<RotateCw className={cn("size-4", refreshing && "animate-spin")} />
					</Button>
					<Button
						variant="outline"
						size="lg"
						isLoading={adding}
						onClick={addGame}
						leftIcon={<FolderPlus className="size-4" />}
					>
						Add a game
					</Button>
				</div>
			</div>

			{state.status === "loading" && <LoadingTiles />}

			{state.status === "error" && (
				<div className="glass max-w-xl">
					<Notice
						icon={<TriangleAlert className="size-5 text-danger" />}
						title="Couldn't read your Steam library"
						action={
							<Button variant="outline" size="sm" leftIcon={<RotateCw className="size-3.5" />} onClick={onRetry}>
								Try again
							</Button>
						}
						details={state.message}
					>
						Your games are fine. Try again, and if it keeps happening, add your game manually.
					</Notice>
				</div>
			)}

			{state.status === "ready" && shown.length > 0 && (
				<ul
					aria-label="Games you can mod"
					onMouseLeave={() => onFocusChange(null)}
					className="grid grid-cols-[repeat(auto-fill,minmax(13rem,1fr))] gap-x-4 gap-y-6"
				>
					{shown.map((game, i) => (
						<li key={game.appId} style={{ animationDelay: `${i * 30}ms` }} className="animate-row-in">
							<GameTile game={game} onHover={() => onFocusChange(game)} />
						</li>
					))}
				</ul>
			)}

			{state.status === "ready" && shown.length === 0 && (
				<EmptyState
					title={q ? "No games found" : state.scan.steamFound ? "No moddable game yet" : "Steam isn't installed here"}
				>
					{q
						? `None of your moddable games matches “${query.trim()}”.`
						: "Add a game by picking its folder, Unalia handles the rest."}
				</EmptyState>
			)}

			{state.status === "ready" && hidden.length > 0 && <NotSupported games={hidden} open={q !== ""} />}
		</section>
	);
}

export function SectionTitle({
	icon,
	count,
	children,
}: {
	icon: ReactNode;
	count?: number;
	children: ReactNode;
}) {
	return (
		<h2 className="flex items-center gap-2 text-sm font-medium tracking-wide text-fg-muted uppercase [&>svg]:size-3.5 [&>svg]:text-fg-subtle">
			{icon}
			{children}
			{count !== undefined && <span className="font-tight text-fg-subtle">{count}</span>}
		</h2>
	);
}

/** Jaquette du jeu, son nom, et ce qui est installé : la carte du backlog d'Unalia. */
function GameTile({ game, onHover }: { game: SteamGame; onHover: () => void }) {
	const installed = useInstalled(game.appId);
	const [cover, setCover] = useState<"loading" | "loaded" | "missing">("loading");
	const count = installed?.length ?? 0;

	return (
		<Link
			href={gameHref(game.appId)}
			onClick={() => rememberGame(game.appId)}
			onMouseEnter={onHover}
			onFocus={onHover}
			className="group block rounded-lg focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent/60"
		>
			<div className="relative aspect-[2/3] overflow-hidden rounded-lg border border-line bg-gradient-to-br from-white/[0.06] to-white/[0.015] transition-[border-color] duration-200 group-hover:border-line-strong">
				{cover !== "loaded" && (
					<span className="absolute inset-0 flex items-center justify-center font-tight text-2xl font-semibold text-fg-subtle">
						{cover === "loading" ? <Skeleton className="size-full rounded-none" /> : initials(game.name)}
					</span>
				)}
				{cover !== "missing" && (
					<img
						src={artworkUrl(game.appId, "cover")}
						alt=""
						loading="lazy"
						decoding="async"
						onLoad={() => setCover("loaded")}
						onError={() => setCover("missing")}
						className={cn(
							"absolute inset-0 size-full object-cover transition-[opacity,scale] duration-500 ease-out group-hover:scale-105",
							cover === "loaded" ? "opacity-100" : "opacity-0",
						)}
					/>
				)}
				<div
					aria-hidden
					className="pointer-events-none absolute inset-x-0 bottom-0 h-1/3 bg-gradient-to-t from-black/70 to-transparent"
				/>
				{count > 0 && (
					<span className="liquid-glass absolute bottom-2 left-2 flex items-center gap-1 rounded-md px-2 py-1 font-tight text-[11px] font-semibold">
						<Package className="size-3" />
						{count === 1 ? "1 mod" : `${count} mods`}
					</span>
				)}
			</div>
			<p className="mt-2 truncate pb-0.5 text-sm font-semibold text-fg transition-colors group-hover:text-accent">
				{game.name}
			</p>
			<p className="truncate text-xs text-fg-subtle">{gameSubtitle(game)}</p>
		</Link>
	);
}

/** Jeux pas encore pris en charge : présents pour qu'on les retrouve, sans encombrer. */
function NotSupported({ games, open }: { games: SteamGame[]; open: boolean }) {
	return (
		<details open={open || undefined} className="group/others mt-10 border-t border-line pt-6">
			<summary className="flex w-fit cursor-pointer list-none items-center gap-2 text-sm font-medium tracking-wide text-fg-subtle uppercase transition-colors select-none hover:text-fg-muted [&::-webkit-details-marker]:hidden">
				<ChevronRight className="size-3.5 transition-transform group-open/others:rotate-90" />
				Not supported yet
				<span className="font-tight">{games.length}</span>
			</summary>
			<p className="mt-2 text-xs text-fg-subtle">
				Unalia can't mod these yet. Support comes one game engine at a time.
			</p>
			<ul className="mt-4 grid grid-cols-[repeat(auto-fill,minmax(15rem,1fr))] gap-2">
				{games.map((game) => (
					<li
						key={game.appId}
						className="flex items-center gap-3 rounded-lg border border-line bg-surface px-2.5 py-2"
					>
						<SmallCover game={game} />
						<span className="min-w-0 flex-1">
							<span className="block truncate text-sm font-medium text-fg-subtle">{game.name}</span>
							<span className="block truncate text-xs text-fg-faint">{gameSubtitle(game)}</span>
						</span>
						<Badge>Soon</Badge>
					</li>
				))}
			</ul>
		</details>
	);
}

function SmallCover({ game }: { game: SteamGame }) {
	const [missing, setMissing] = useState(false);
	return (
		<span className="relative flex h-12 w-8 shrink-0 items-center justify-center overflow-hidden rounded-md border border-line bg-surface font-tight text-[11px] font-semibold text-fg-subtle opacity-60 grayscale">
			{initials(game.name)}
			{!missing && (
				<img
					src={artworkUrl(game.appId, "cover")}
					alt=""
					loading="lazy"
					onError={() => setMissing(true)}
					className="absolute inset-0 size-full object-cover"
				/>
			)}
		</span>
	);
}

function EmptyState({ title, children }: { title: string; children: ReactNode }) {
	return (
		<div className="flex flex-col items-center justify-center gap-1 rounded-lg border border-dashed border-line bg-white/[0.02] px-6 py-10 text-center">
			<Gamepad2 className="mb-1 size-6 text-fg-faint" strokeWidth={1.5} />
			<p className="text-sm font-medium text-fg-muted">{title}</p>
			<p className="text-xs text-fg-subtle">{children}</p>
		</div>
	);
}

function LoadingTiles() {
	return (
		<div
			role="status"
			aria-label="Looking for your games"
			className="grid grid-cols-[repeat(auto-fill,minmax(13rem,1fr))] gap-x-4 gap-y-6"
		>
			{[0, 1, 2, 3, 4, 5].map((i) => (
				<div key={i}>
					<Skeleton className="aspect-[2/3] w-full rounded-lg" />
					<Skeleton className="mt-2 h-3.5 w-3/4" />
					<Skeleton className="mt-1.5 h-3 w-1/2" />
				</div>
			))}
		</div>
	);
}
