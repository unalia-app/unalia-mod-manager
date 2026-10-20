"use client";

import { SearchField, cn } from "@modalia/ui";
import { Check, ChevronDown, LayoutGrid } from "lucide-react";
import Link from "next/link";
import { useRouter } from "next/navigation";
import { type KeyboardEvent, useEffect, useMemo, useRef, useState } from "react";

import { GameCover } from "@/components/game-cover";
import { type SteamGame, isModdable } from "@/lib/engine";
import { gameHref, loadGames, rememberGame, withRecentsFirst } from "@/lib/games-store";
import { normalize } from "@/lib/text";

const LIMIT = 8;

/** Sélecteur de jeu de l'en-tête (SPEC §3.2) : jeu actif, recherche, récents en premier. */
export function GameSwitcher({ current }: { current: SteamGame }) {
	const router = useRouter();
	const [open, setOpen] = useState(false);
	const [query, setQuery] = useState("");
	const [games, setGames] = useState<SteamGame[]>([]);
	const [active, setActive] = useState(0);
	const root = useRef<HTMLDivElement>(null);

	useEffect(() => {
		if (!open) return;
		loadGames().then(({ games }) => setGames(withRecentsFirst(games.filter(isModdable))), () => setGames([]));
		setQuery("");
		const close = (e: PointerEvent) => !root.current?.contains(e.target as Node) && setOpen(false);
		window.addEventListener("pointerdown", close);
		return () => window.removeEventListener("pointerdown", close);
	}, [open]);

	const results = useMemo(() => {
		const q = normalize(query);
		return (q ? games.filter((g) => normalize(g.name).includes(q)) : games).slice(0, LIMIT);
	}, [games, query]);

	useEffect(() => setActive(0), [results]);

	const select = (game: SteamGame) => {
		setOpen(false);
		rememberGame(game.appId);
		if (game.appId !== current.appId) router.push(gameHref(game.appId));
	};

	const onKeyDown = (e: KeyboardEvent) => {
		if (e.key === "Escape") setOpen(false);
		if (!results.length) return;
		if (e.key === "ArrowDown" || e.key === "ArrowUp") {
			e.preventDefault();
			const step = e.key === "ArrowDown" ? 1 : -1;
			setActive((i) => (i + step + results.length) % results.length);
		} else if (e.key === "Enter") {
			const game = results[active];
			if (game) select(game);
		}
	};

	return (
		<div ref={root} className="relative" onKeyDown={onKeyDown}>
			<button
				type="button"
				aria-haspopup="listbox"
				aria-expanded={open}
				onClick={() => setOpen((o) => !o)}
				className={cn(
					"flex h-10 cursor-pointer items-center gap-2.5 rounded-lg border py-1 pr-2.5 pl-1.5 transition-colors duration-200",
					open ? "border-line-strong bg-surface-strong" : "border-transparent hover:border-line hover:bg-surface-hover",
				)}
			>
				<GameCover game={current} className="h-8 w-[22px] rounded-[5px]" />
				<span className="max-w-56 truncate text-sm font-medium">{current.name}</span>
				<ChevronDown
					className={cn("size-4 text-fg-subtle transition-transform duration-200", open && "rotate-180")}
				/>
			</button>

			{open && (
				<div className="absolute top-full left-0 z-30 mt-2 w-80 origin-top-left animate-row-in rounded-xl border border-line bg-[#141417]/95 p-1.5 shadow-2xl backdrop-blur-xl">
					<SearchField
						autoFocus
						value={query}
						onChange={(e) => setQuery(e.target.value)}
						placeholder="Changer de jeu"
						aria-label="Changer de jeu"
						className="h-10 hover:translate-y-0"
					/>
					<ul role="listbox" aria-label="Jeux" className="mt-1.5">
						{results.map((game, i) => (
							<li key={game.appId}>
								<button
									type="button"
									role="option"
									aria-selected={game.appId === current.appId}
									onMouseEnter={() => setActive(i)}
									onClick={() => select(game)}
									className={cn(
										"flex w-full cursor-pointer items-center gap-2.5 rounded-lg px-2 py-1.5 text-left",
										i === active && "bg-surface-hover",
									)}
								>
									<GameCover game={game} className="h-9 w-6 rounded-[5px]" />
									<span className="min-w-0 flex-1 truncate text-sm">{game.name}</span>
									{game.appId === current.appId && <Check className="size-4 text-accent" />}
								</button>
							</li>
						))}
						{results.length === 0 && (
							<li className="px-2 py-3 text-xs text-fg-subtle">
								{query ? `Aucun jeu moddable ne correspond à « ${query.trim()} ».` : "Recherche de tes jeux…"}
							</li>
						)}
					</ul>
					<Link
						href="/"
						className="mt-1 flex items-center gap-2 rounded-lg border-t border-line px-2 pt-2.5 pb-1.5 text-xs font-medium text-fg-subtle transition-colors hover:text-fg"
					>
						<LayoutGrid className="size-3.5" />
						Tous mes jeux
					</Link>
				</div>
			)}
		</div>
	);
}
