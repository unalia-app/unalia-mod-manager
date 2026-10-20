"use client";

import { Button, SearchField, Segmented } from "@modalia/ui";
import { PackageSearch, RotateCw, TriangleAlert } from "lucide-react";
import { useEffect, useRef, useState } from "react";

import { ModCard, ModCardSkeleton } from "@/components/game/mod-card";
import { Notice } from "@/components/notice";
import { type ModSort, type ModSummary, searchMods } from "@/lib/engine";
import { compactNumber } from "@/lib/format";

const SORTS: { value: ModSort; label: string }[] = [
	{ value: "popular", label: "Popular" },
	{ value: "newest", label: "Newest" },
	{ value: "updated", label: "Updated" },
];

const GRID = "grid grid-cols-[repeat(auto-fill,minmax(11.5rem,1fr))] gap-4";

type Results = {
	items: ModSummary[];
	total: number;
	page: number;
	hasMore: boolean;
};

/** Onglet Découvrir : recherche, tri et grille qui se charge au défilement. */
export function Discover({ community, appId }: { community: string; appId: number }) {
	const [text, setText] = useState("");
	const query = useDebounced(text.trim(), 250);
	const [sort, setSort] = useState<ModSort>("popular");
	const [results, setResults] = useState<Results | null>(null);
	const [loadingMore, setLoadingMore] = useState(false);
	const [error, setError] = useState<string | null>(null);
	const [attempt, setAttempt] = useState(0);
	// Chaque nouvelle recherche invalide les réponses encore en vol.
	const generation = useRef(0);


	useEffect(() => {
		const current = ++generation.current;
		setResults(null);
		setError(null);
		searchMods(community, query, sort, 1).then(
			(page) => current === generation.current && setResults({ ...page, page: 1 }),
			(e: unknown) => current === generation.current && setError(String(e)),
		);
	}, [community, query, sort, attempt]);

	const loadMore = () => {
		if (!results?.hasMore || loadingMore) return;
		const current = generation.current;
		const next = results.page + 1;
		setLoadingMore(true);
		searchMods(community, query, sort, next)
			.then(
				(page) => {
					if (current !== generation.current) return;
					setResults((r) => {
						if (!r) return r;
						// Le classement peut bouger entre deux pages : pas de doublon.
						const seen = new Set(r.items.map((m) => m.id));
						return {
							items: [...r.items, ...page.items.filter((m) => !seen.has(m.id))],
							total: page.total,
							page: next,
							hasMore: page.hasMore,
						};
					});
				},
				(e: unknown) => current === generation.current && setError(String(e)),
			)
			.finally(() => setLoadingMore(false));
	};

	const sentinel = useInView<HTMLDivElement>(loadMore, results?.items.length);

	return (
		<div>
			<div className="flex flex-wrap items-center gap-3">
				<SearchField
					value={text}
					onChange={(e) => setText(e.target.value)}
					placeholder="Search mods"
					aria-label="Search mods"
					className="h-10 max-w-sm flex-1"
				/>
				<Segmented label="Sort mods" items={SORTS} value={sort} onChange={setSort} />
				{results && (
					<span className="ml-auto font-tight text-xs text-fg-subtle">
						{compactNumber(results.total)} mod{results.total > 1 ? "s" : ""}
					</span>
				)}
			</div>

			<div className="mt-6">
				{error && !results && (
					<div className="glass max-w-xl">
						<Notice
							icon={<TriangleAlert className="size-5 text-danger" />}
							title="Couldn't load mods"
							action={
								<Button
									variant="outline"
									size="sm"
									leftIcon={<RotateCw className="size-3.5" />}
									onClick={() => setAttempt((a) => a + 1)}
								>
									Try again
								</Button>
							}
							details={error}
						>
							Check your connection. Mods you already installed are not affected.
						</Notice>
					</div>
				)}

				{!error && !results && (
					<div className={GRID}>
						{Array.from({ length: 12 }, (_, i) => (
							<ModCardSkeleton key={i} />
						))}
					</div>
				)}

				{results && results.items.length === 0 && (
					<div className="glass max-w-xl">
						<Notice icon={<PackageSearch className="size-5 text-fg-muted" />} title="No mods found">
							{query
								? `No mod matches “${query}”. Try fewer words.`
								: "This game has no mods on Thunderstore yet."}
						</Notice>
					</div>
				)}

				{results && results.items.length > 0 && (
					<>
						<div className={GRID}>
							{results.items.map((mod, i) => (
								<ModCard key={mod.id} mod={mod} index={i} appId={appId} />
							))}
							{loadingMore && Array.from({ length: 4 }, (_, i) => <ModCardSkeleton key={`more-${i}`} />)}
						</div>
						{results.hasMore && <div ref={sentinel} className="h-px" />}
						{error && (
							<p className="mt-6 text-center text-xs text-fg-subtle">
								Couldn't load more mods.{" "}
								<button type="button" className="cursor-pointer text-fg underline" onClick={loadMore}>
									Try again
								</button>
							</p>
						)}
					</>
				)}
			</div>

		</div>
	);
}

function useDebounced<T>(value: T, delay: number): T {
	const [debounced, setDebounced] = useState(value);
	useEffect(() => {
		const timer = window.setTimeout(() => setDebounced(value), delay);
		return () => window.clearTimeout(timer);
	}, [value, delay]);
	return debounced;
}

/**
 * Appelle `onVisible` quand l'élément approche de l'écran (600 px d'avance).
 * L'observateur est recréé quand `version` change : si l'élément est resté
 * visible après un chargement, il se redéclenche quand même.
 */
function useInView<T extends Element>(onVisible: () => void, version: unknown) {
	const callback = useRef(onVisible);
	callback.current = onVisible;
	const [node, setNode] = useState<T | null>(null);

	useEffect(() => {
		if (!node) return;
		const observer = new IntersectionObserver((entries) => entries[0]?.isIntersecting && callback.current(), {
			rootMargin: "600px 0px",
		});
		observer.observe(node);
		return () => observer.disconnect();
	}, [node, version]);

	return setNode;
}
