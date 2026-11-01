"use client";

import {
	Button,
	Combobox,
	ComboboxContent,
	ComboboxEmpty,
	type ComboboxFilter,
	ComboboxInput,
	ComboboxItem,
	ComboboxList,
	ComboboxTrigger,
	SearchField,
	Segmented,
} from "@modalia/ui";
import { PackageSearch, RotateCw, TriangleAlert } from "lucide-react";
import { useEffect, useRef, useState } from "react";

import { ModCard, ModCardSkeleton } from "@/components/game/mod-card";
import { Notice } from "@/components/notice";
import {
	type ModCategory,
	type ModSort,
	type ModSummary,
	SOURCE_LABEL,
	hexiumCommunities,
	modCategories,
	searchMods,
} from "@/lib/engine";
import { compactNumber } from "@/lib/format";

const SORTS: { value: ModSort; label: string }[] = [
	{ value: "popular", label: "Popular" },
	{ value: "downloads", label: "Downloads" },
	{ value: "newest", label: "Newest" },
	{ value: "updated", label: "Updated" },
];

const ALL_CATEGORIES = "all";

// Coming back from a mod's page rebuilds the screen: each game keeps its
// view until the app closes.
const lastViews = new Map<string, { source: Source; text: string; sort: ModSort; category: string }>();

/** On the name alone: a Thunderstore category's value is a number. */
const matchesName: ComboboxFilter = (_, query, keywords) =>
	keywords.join(" ").toLowerCase().includes(query.trim().toLowerCase());

type Source = "thunderstore" | "hexium";

const SOURCES: { value: Source; label: string }[] = [
	{ value: "thunderstore", label: SOURCE_LABEL.thunderstore },
	{ value: "hexium", label: SOURCE_LABEL.hexium },
];

// Asked once per session: the list barely moves.
let hexiumList: Promise<string[]> | null = null;

/** The game is also on Hexium: Discover offers both sources. */
export function useOnHexium(community: string): boolean {
	const [onHexium, setOnHexium] = useState(false);
	useEffect(() => {
		if (community.startsWith("nexus:")) return;
		let current = true;
		hexiumList ??= hexiumCommunities().catch(() => {
			hexiumList = null;
			return [];
		});
		void hexiumList.then((list) => current && setOnHexium(list.includes(community)));
		return () => {
			current = false;
		};
	}, [community]);
	return onHexium;
}

const GRID = "grid grid-cols-[repeat(auto-fill,minmax(15rem,1fr))] gap-4";

type Results = {
	items: ModSummary[];
	total: number;
	page: number;
	hasMore: boolean;
};

/**
 * Discover tab: search, category, sort and a grid that loads on scroll. A game also
 * on Hexium gets a source switch: each source keeps its own list.
 */
export function Discover({ community: gameCommunity, appId }: { community: string; appId: number }) {
	const onHexium = useOnHexium(gameCommunity);
	const saved = lastViews.get(gameCommunity);
	const [picked, setPicked] = useState<Source>(saved?.source ?? "thunderstore");
	const source: Source = onHexium ? picked : "thunderstore";
	const community = source === "hexium" ? `hexium:${gameCommunity}` : gameCommunity;
	const sourceLabel = gameCommunity.startsWith("nexus:") ? "Nexus Mods" : SOURCE_LABEL[source];
	const [text, setText] = useState(saved?.text ?? "");
	const query = useDebounced(text.trim(), 250);
	const [sort, setSort] = useState<ModSort>(saved?.sort ?? "popular");
	const [categories, setCategories] = useState<ModCategory[]>([]);
	const [pickedCategory, setPickedCategory] = useState(saved?.category ?? ALL_CATEGORIES);
	const category = pickedCategory === ALL_CATEGORIES ? null : pickedCategory;
	const [results, setResults] = useState<Results | null>(null);
	const [loadingMore, setLoadingMore] = useState(false);
	const [error, setError] = useState<string | null>(null);
	const [attempt, setAttempt] = useState(0);
	// Each new search invalidates the answers still in flight.
	const generation = useRef(0);

	useEffect(() => {
		lastViews.set(gameCommunity, { source: picked, text, sort, category: pickedCategory });
	}, [gameCommunity, picked, text, sort, pickedCategory]);

	// Each source has its own. Without them, the search works all the same.
	useEffect(() => {
		let current = true;
		setCategories([]);
		modCategories(community).then(
			(list) => current && setCategories([...list].sort((a, b) => a.name.localeCompare(b.name))),
			() => {},
		);
		return () => {
			current = false;
		};
	}, [community]);

	useEffect(() => {
		const current = ++generation.current;
		setResults(null);
		setError(null);
		searchMods(community, query, sort, 1, category).then(
			(page) => current === generation.current && setResults({ ...page, page: 1 }),
			(e: unknown) => current === generation.current && setError(String(e)),
		);
	}, [community, query, sort, category, attempt]);

	const loadMore = () => {
		if (!results?.hasMore || loadingMore) return;
		const current = generation.current;
		const next = results.page + 1;
		setLoadingMore(true);
		searchMods(community, query, sort, next, category)
			.then(
				(page) => {
					if (current !== generation.current) return;
					setResults((r) => {
						if (!r) return r;
						// The ranking can move between two pages: no duplicates.
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
	// Size of the first page: only its cards get the staggered entrance.
	const firstPage = useRef(0);
	if (results?.page === 1) firstPage.current = results.items.length;
	const firstPageSize = firstPage.current;

	return (
		<div>
			<div className="flex flex-wrap items-center gap-3">
				{onHexium && (
					<Segmented
						label="Mod source"
						items={SOURCES}
						value={source}
						onChange={(next) => {
							setPicked(next);
							setPickedCategory(ALL_CATEGORIES);
						}}
					/>
				)}
				<SearchField
					value={text}
					onChange={(e) => setText(e.target.value)}
					placeholder="Search mods"
					aria-label="Search mods"
					className="h-10 max-w-sm flex-1"
				/>
				{categories.length > 0 && (
					<Combobox value={pickedCategory} onValueChange={setPickedCategory} filter={matchesName} className="w-56">
						<ComboboxTrigger onClear={category ? () => setPickedCategory(ALL_CATEGORIES) : undefined}>
							<ComboboxInput aria-label="Filter by category" placeholder="Search categories" />
						</ComboboxTrigger>
						<ComboboxContent>
							<ComboboxList ariaLabel="Categories">
								<ComboboxItem value={ALL_CATEGORIES}>All categories</ComboboxItem>
								{categories.map((c) => (
									<ComboboxItem key={c.id} value={c.id}>
										{c.name}
									</ComboboxItem>
								))}
								<ComboboxEmpty>No category found.</ComboboxEmpty>
							</ComboboxList>
						</ComboboxContent>
					</Combobox>
				)}
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
								: category
									? "No mod in this category."
									: `This game has no mods on ${sourceLabel} yet.`}
						</Notice>
					</div>
				)}

				{results && results.items.length > 0 && (
					<>
						<div className={GRID}>
							{results.items.map((mod, i) => (
								<ModCard key={mod.id} mod={mod} index={i} appId={appId} animated={i < firstPageSize} />
							))}
							{/* Always there while pages remain: new cards are inserted
							    before it, nothing appears or disappears. */}
							{results.hasMore &&
								Array.from({ length: 4 }, (_, i) => <ModCardSkeleton key={`more-${i}`} />)}
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
 * Calls `onVisible` when the element gets close to the screen (600 px
 * ahead). The observer is recreated when `version` changes: if the element
 * stayed visible after a load, it fires again anyway.
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
