"use client";

import { Badge, Button, Kbd, SearchField, Skeleton, cn } from "@modalia/ui";
import { ArrowRight, FolderPlus, Gamepad2, RotateCw, TriangleAlert } from "lucide-react";
import { useRouter } from "next/navigation";
import { type KeyboardEvent, useCallback, useEffect, useMemo, useState } from "react";

import { GameCover } from "@/components/game-cover";
import { Notice } from "@/components/notice";
import { type GameScan, type SteamGame, gameSubtitle, isModdable, splitError } from "@/lib/engine";
import { toast } from "@/components/toaster";
import { pickAndAddGame } from "@/lib/add-game";
import { gameHref, loadGames, rememberGame, withRecentsFirst } from "@/lib/games-store";
import { normalize } from "@/lib/text";

/** Jamais une liste de 1 000 jeux d'emblée (SPEC §3.1). */
const SUGGESTION_COUNT = 5;
const SEARCH_LIMIT = 8;

type ScanState =
	| { status: "loading" }
	| { status: "ready"; scan: GameScan }
	| { status: "error"; message: string };

export function GamePicker({
	onFocusChange,
}: {
	/** Jeu mis en avant par l'utilisateur (survol, flèches), `null` sinon. */
	onFocusChange: (game: SteamGame | null) => void;
}) {
	const [state, setState] = useState<ScanState>({ status: "loading" });
	const [query, setQuery] = useState("");
	const [showAll, setShowAll] = useState(false);
	const [active, setActive] = useState(0);
	// La première ligne est active par défaut pour Entrée, mais l'ambiance ne
	// change que si l'utilisateur désigne lui-même un jeu.
	const [engaged, setEngaged] = useState(false);

	const router = useRouter();
	const [adding, setAdding] = useState(false);

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

	const scan = useCallback((fresh = false) => {
		setState({ status: "loading" });
		loadGames({ fresh }).then(
			(result) => setState({ status: "ready", scan: result }),
			(error: unknown) => setState({ status: "error", message: String(error) }),
		);
	}, []);

	useEffect(() => scan(), [scan]);

	const games = state.status === "ready" ? state.scan.games : [];
	const moddable = useMemo(() => withRecentsFirst(games.filter(isModdable)), [games]);

	// Sans recherche : les jeux moddables. Avec : tous les jeux qui
	// correspondent, moddables d'abord, pour expliquer pourquoi un jeu
	// attendu n'est pas proposé.
	const results = useMemo(() => {
		const q = normalize(query);
		if (!q) return showAll ? moddable : moddable.slice(0, SUGGESTION_COUNT);
		return games
			.filter((g) => normalize(g.name).includes(q))
			.sort((a, b) => Number(isModdable(b)) - Number(isModdable(a)))
			.slice(0, SEARCH_LIMIT);
	}, [games, moddable, query, showAll]);

	useEffect(() => setActive(0), [results]);

	useEffect(() => {
		const game = engaged ? results[active] : undefined;
		onFocusChange(game && isModdable(game) ? game : null);
	}, [engaged, results, active, onFocusChange]);

	const select = (game: SteamGame) => {
		if (!isModdable(game)) return;
		rememberGame(game.appId);
		router.push(gameHref(game.appId));
	};

	const onKeyDown = (event: KeyboardEvent<HTMLInputElement>) => {
		if (!results.length) return;
		if (event.key === "ArrowDown" || event.key === "ArrowUp") {
			event.preventDefault();
			setEngaged(true);
			const step = event.key === "ArrowDown" ? 1 : -1;
			setActive((i) => (i + step + results.length) % results.length);
		} else if (event.key === "Enter") {
			const game = results[active];
			if (game) select(game);
		}
	};

	const hiddenCount = moddable.length - SUGGESTION_COUNT;

	return (
		<div className="w-full">
			<SearchField
				autoFocus
				value={query}
				onChange={(e) => setQuery(e.target.value)}
				onKeyDown={onKeyDown}
				placeholder="Search your installed games"
				aria-label="Search games"
				aria-controls="game-results"
				className="h-14"
				trailing={results.length > 0 && <Kbd>↵</Kbd>}
			/>

			<div id="game-results" className="glass mt-3 p-1.5">
				{state.status === "loading" && <LoadingRows />}

				{state.status === "error" && (
					<Notice
						icon={<TriangleAlert className="size-5 text-danger" />}
						title="Couldn't read your Steam library"
						action={
							<Button variant="outline" size="sm" leftIcon={<RotateCw className="size-3.5" />} onClick={() => scan(true)}>
								Try again
							</Button>
						}
						details={state.message}
					>
						Your games are fine. Try again, and if it keeps happening, add your game manually.
					</Notice>
				)}

				{state.status === "ready" && !state.scan.steamFound && games.length === 0 && (
					<Notice
						icon={<Gamepad2 className="size-5 text-fg-muted" />}
						title="Steam isn't installed here"
						action={
							<Button size="sm" isLoading={adding} onClick={addGame} leftIcon={<FolderPlus className="size-3.5" />}>
								Add a game
							</Button>
						}
					>
						Add a game by picking its folder, Modalia handles the rest.
					</Notice>
				)}

				{state.status === "ready" && (state.scan.steamFound || games.length > 0) && results.length === 0 && (
					<Notice icon={<Gamepad2 className="size-5 text-fg-muted" />} title={query ? "No games found" : "No moddable games yet"}>
						{query
							? `No installed game matches “${query.trim()}”.`
							: "Modalia starts with games that use BepInEx. More are coming soon."}
					</Notice>
				)}

				{state.status === "ready" && results.length > 0 && (
					<ul aria-label="Games" onMouseLeave={() => setEngaged(false)}>
						{results.map((game, i) => (
							<li key={game.appId}>
								<GameRow
									game={game}
									active={i === active}
									index={i}
									onHover={() => {
										setActive(i);
										setEngaged(true);
									}}
									onSelect={() => select(game)}
								/>
							</li>
						))}
					</ul>
				)}
			</div>

			<div className="mt-4 flex items-center justify-between gap-4">
				{!query && hiddenCount > 0 ? (
					<button
						type="button"
						onClick={() => setShowAll((v) => !v)}
						className="cursor-pointer text-sm font-medium text-fg-subtle transition-colors hover:text-fg"
					>
						{showAll ? "Show less" : `See all my moddable games (${moddable.length})`}
					</button>
				) : (
					<span />
				)}
				<Button
					variant="secondary"
					size="sm"
					isLoading={adding}
					onClick={addGame}
					leftIcon={<FolderPlus className="size-3.5" />}
				>
					Add a game manually
				</Button>
			</div>
		</div>
	);
}

function GameRow({
	game,
	active,
	index,
	onHover,
	onSelect,
}: {
	game: SteamGame;
	active: boolean;
	index: number;
	onHover: () => void;
	onSelect: () => void;
}) {
	const moddable = isModdable(game);

	return (
		<button
			type="button"
			onMouseEnter={onHover}
			onClick={onSelect}
			aria-disabled={!moddable || undefined}
			aria-current={active || undefined}
			style={{ animationDelay: `${index * 30}ms` }}
			className={cn(
				"group/row flex w-full animate-row-in items-center gap-3 rounded-lg px-2.5 py-2 text-left transition-colors duration-150",
				moddable ? "cursor-pointer" : "cursor-default",
				active && "bg-surface-hover",
			)}
		>
			<GameCover game={game} dimmed={!moddable} />
			<span className="min-w-0 flex-1">
				<span className={cn("block truncate text-sm font-medium", moddable ? "text-fg" : "text-fg-subtle")}>
					{game.name}
				</span>
				<span className="block truncate text-xs text-fg-subtle">{gameSubtitle(game)}
					{game.source === "manual" && " · Added manually"}
				</span>
			</span>
			{moddable ? (
				<ArrowRight
					className={cn(
						"size-4 shrink-0 text-accent transition-[opacity,translate] duration-200",
						active ? "translate-x-0 opacity-100" : "-translate-x-1 opacity-0",
					)}
				/>
			) : (
				<Badge>Soon</Badge>
			)}
		</button>
	);
}

function LoadingRows() {
	return (
		<div role="status" aria-label="Looking for your games">
			{[0, 1, 2].map((i) => (
				<div key={i} className="flex items-center gap-3 px-2.5 py-2">
					<Skeleton className="h-12 w-8" />
					<div className="flex-1 space-y-2">
						<Skeleton className="h-3.5 w-2/5" />
						<Skeleton className="h-3 w-1/5" />
					</div>
				</div>
			))}
		</div>
	);
}
