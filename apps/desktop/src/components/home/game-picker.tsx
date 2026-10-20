"use client";

import { Badge, Button, Kbd, SearchField, Skeleton, cn } from "@modalia/ui";
import { ArrowRight, FolderPlus, Gamepad2, RotateCw, TriangleAlert } from "lucide-react";
import { type KeyboardEvent, type ReactNode, useCallback, useEffect, useMemo, useState } from "react";

import { ENGINE_LABEL, type GameScan, type SteamGame, isModdable, listGames } from "@/lib/engine";

/** Jamais une liste de 1 000 jeux d'emblée (SPEC §3.1). */
const SUGGESTION_COUNT = 5;

type ScanState =
	| { status: "loading" }
	| { status: "ready"; scan: GameScan }
	| { status: "error"; message: string };

export function GamePicker() {
	const [state, setState] = useState<ScanState>({ status: "loading" });
	const [query, setQuery] = useState("");
	const [showAll, setShowAll] = useState(false);
	const [active, setActive] = useState(0);

	const scan = useCallback(() => {
		setState({ status: "loading" });
		listGames().then(
			(result) => setState({ status: "ready", scan: result }),
			(error: unknown) => setState({ status: "error", message: String(error) }),
		);
	}, []);

	useEffect(scan, [scan]);

	const games = state.status === "ready" ? state.scan.games : [];
	const moddable = useMemo(() => games.filter(isModdable), [games]);

	// Sans recherche : les jeux moddables. Avec : tous les jeux qui
	// correspondent, moddables d'abord, pour expliquer pourquoi un jeu
	// attendu n'est pas proposé.
	const results = useMemo(() => {
		const q = normalize(query);
		if (!q) return showAll ? moddable : moddable.slice(0, SUGGESTION_COUNT);
		return games
			.filter((g) => normalize(g.name).includes(q))
			.sort((a, b) => Number(isModdable(b)) - Number(isModdable(a)));
	}, [games, moddable, query, showAll]);

	useEffect(() => setActive(0), [results]);

	const select = (game: SteamGame) => {
		if (!isModdable(game)) return;
		// Branché quand l'écran du jeu (Découvrir / Mes mods) existera.
	};

	const onKeyDown = (event: KeyboardEvent<HTMLInputElement>) => {
		if (!results.length) return;
		if (event.key === "ArrowDown" || event.key === "ArrowUp") {
			event.preventDefault();
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
				placeholder="Rechercher parmi tes jeux installés"
				aria-label="Rechercher un jeu"
				aria-controls="game-results"
				className="h-14"
				trailing={results.length > 0 && <Kbd>↵</Kbd>}
			/>

			<div id="game-results" className="glass mt-3 p-1.5">
				{state.status === "loading" && <LoadingRows />}

				{state.status === "error" && (
					<Notice
						icon={<TriangleAlert className="size-5 text-danger" />}
						title="Impossible de lire ta bibliothèque Steam"
						action={
							<Button variant="outline" size="sm" leftIcon={<RotateCw className="size-3.5" />} onClick={scan}>
								Réessayer
							</Button>
						}
						details={state.message}
					>
						Ton jeu n'est pas en cause. Réessaie, et si ça persiste, ajoute ton jeu à la main.
					</Notice>
				)}

				{state.status === "ready" && !state.scan.steamFound && (
					<Notice icon={<Gamepad2 className="size-5 text-fg-muted" />} title="Steam n'est pas installé ici">
						Ajoute un jeu en choisissant son dossier, Modalia s'occupe du reste.
					</Notice>
				)}

				{state.status === "ready" && state.scan.steamFound && results.length === 0 && (
					<Notice icon={<Gamepad2 className="size-5 text-fg-muted" />} title={query ? "Aucun jeu trouvé" : "Aucun jeu moddable pour l'instant"}>
						{query
							? `Aucun jeu installé ne correspond à « ${query.trim()} ».`
							: "Modalia commence par les jeux Unity. D'autres moteurs arrivent bientôt."}
					</Notice>
				)}

				{state.status === "ready" && results.length > 0 && (
					<ul aria-label="Jeux">
						{results.map((game, i) => (
							<li key={game.appId}>
								<GameRow
									game={game}
									active={i === active}
									index={i}
									onHover={() => setActive(i)}
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
						{showAll ? "Afficher moins" : `Voir tous mes jeux moddables (${moddable.length})`}
					</button>
				) : (
					<span />
				)}
				<Button
					variant="secondary"
					size="sm"
					disabled
					title="Bientôt disponible"
					leftIcon={<FolderPlus className="size-3.5" />}
				>
					Ajouter un jeu manuellement
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
			<GameTile name={game.name} dimmed={!moddable} />
			<span className="min-w-0 flex-1">
				<span className={cn("block truncate text-sm font-medium", moddable ? "text-fg" : "text-fg-subtle")}>
					{game.name}
				</span>
				<span className="block truncate text-xs text-fg-subtle">{ENGINE_LABEL[game.engine]}</span>
			</span>
			{moddable ? (
				<ArrowRight
					className={cn(
						"size-4 shrink-0 text-fg-muted transition-[opacity,transform] duration-200",
						active ? "translate-x-0 opacity-100" : "-translate-x-1 opacity-0",
					)}
				/>
			) : (
				<Badge>Bientôt</Badge>
			)}
		</button>
	);
}

/** Vignette provisoire, en attendant les jaquettes du cache local de Steam. */
function GameTile({ name, dimmed }: { name: string; dimmed: boolean }) {
	return (
		<span
			aria-hidden
			className={cn(
				"flex size-10 shrink-0 items-center justify-center rounded-md border border-line bg-gradient-to-br from-white/[0.08] to-white/[0.02] font-tight text-sm font-semibold text-fg-muted",
				dimmed && "opacity-50",
			)}
		>
			{initials(name)}
		</span>
	);
}

function LoadingRows() {
	return (
		<div role="status" aria-label="Recherche de tes jeux">
			{[0, 1, 2].map((i) => (
				<div key={i} className="flex items-center gap-3 px-2.5 py-2">
					<Skeleton className="size-10" />
					<div className="flex-1 space-y-2">
						<Skeleton className="h-3.5 w-2/5" />
						<Skeleton className="h-3 w-1/5" />
					</div>
				</div>
			))}
		</div>
	);
}

function Notice({
	icon,
	title,
	children,
	action,
	details,
}: {
	icon: ReactNode;
	title: string;
	children: ReactNode;
	action?: ReactNode;
	details?: string;
}) {
	return (
		<div className="flex animate-row-in gap-3 px-3 py-3.5">
			<span className="flex size-10 shrink-0 items-center justify-center rounded-xl border border-line bg-surface">
				{icon}
			</span>
			<div className="min-w-0 flex-1">
				<p className="text-sm font-medium text-fg">{title}</p>
				<p className="mt-0.5 text-xs leading-relaxed text-fg-subtle">{children}</p>
				{action && <div className="mt-3">{action}</div>}
				{details && (
					<details className="mt-3 text-xs text-fg-faint">
						<summary className="cursor-pointer select-none transition-colors hover:text-fg-subtle">
							Détails techniques
						</summary>
						<pre className="mt-2 overflow-x-auto whitespace-pre-wrap rounded-md border border-line bg-black/30 p-2.5 font-mono text-[11px] text-fg-subtle">
							{details}
						</pre>
					</details>
				)}
			</div>
		</div>
	);
}

/** Recherche tolérante : casse, accents et ponctuation ignorés. */
function normalize(text: string): string {
	return text
		.normalize("NFD")
		.replace(/\p{Diacritic}/gu, "")
		.toLowerCase()
		.replace(/[^\p{L}\p{N}]+/gu, " ")
		.trim();
}

function initials(name: string): string {
	const words = normalize(name).split(" ").filter(Boolean);
	return words
		.slice(0, 2)
		.map((w) => w[0]?.toUpperCase())
		.join("");
}
