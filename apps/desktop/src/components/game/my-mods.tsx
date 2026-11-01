"use client";

import {
	Badge,
	Button,
	SearchField,
	Segmented,
	Select,
	SelectContent,
	SelectItem,
	SelectTrigger,
	SelectValue,
	Skeleton,
	Spinner,
	Switch,
	cn,
} from "@modalia/ui";
import {
	ArrowUp,
	Check,
	Copy,
	ExternalLink,
	LayoutGrid,
	Layers,
	Package,
	PackageSearch,
	Pause,
	Play,
	RotateCw,
	Rows3,
	SquareCheck,
	Trash2,
	X,
} from "lucide-react";
import { useRouter } from "next/navigation";
import { type ReactNode, useCallback, useEffect, useRef, useState } from "react";

import { progressDetail, progressLabel } from "@/components/mod/install-button";
import { ModNeeds } from "@/components/mod/mod-needs";
import { uninstallManyWithUndo, uninstallWithUndo } from "@/components/mod/uninstall";
import { ImportButton, ShareButton } from "@/components/game/share-mods";
import { Marquee } from "@/components/marquee";
import { Notice } from "@/components/notice";
import { toast } from "@/components/toaster";
import { useAdultBlur } from "@/lib/adult-content";
import {
	type InstalledMod,
	type PausedDownload,
	type SteamGame,
	discardDownload,
	nexusModUrl,
	pausedDownloads,
	splitError,
} from "@/lib/engine";
import { bytes, relativeDate } from "@/lib/format";
import { useGameStatus, usePendingOp } from "@/lib/game-status";
import { modHref } from "@/lib/games-store";
import { copyLink, openExternal } from "@/lib/open";
import {
	type ActiveInstall,
	cancel,
	install,
	overallProgress,
	revert,
	runUpdateCheck,
	toggle,
	toggleMany,
	updateAll,
	useActiveInstall,
	useActiveInstalls,
	useDeprecated,
	useInstalled,
	useModUpdate,
	useUpdateCheck,
	useUpdates,
} from "@/lib/installs";

type MyModsSort = "name" | "recent" | "off";

const SORTS: { value: MyModsSort; label: string }[] = [
	{ value: "name", label: "Name" },
	{ value: "recent", label: "Recent" },
	{ value: "off", label: "Off first" },
];

type MyModsLayout = "list" | "compact";

const LAYOUTS: { value: MyModsLayout; label: ReactNode }[] = [
	{ value: "list", label: <Rows3 className="size-3.5" aria-label="List" /> },
	{ value: "compact", label: <LayoutGrid className="size-3.5" aria-label="Compact" /> },
];

const LAYOUT_KEY = "modalia.myMods.layout";

function storedLayout(): MyModsLayout {
	try {
		return localStorage.getItem(LAYOUT_KEY) === "compact" ? "compact" : "list";
	} catch {
		return "list";
	}
}

function fail(error: unknown) {
	const { message, details } = splitError(error);
	toast({ title: message, details: details ?? undefined, tone: "error" });
}

// Coming back from a mod's page rebuilds the screen: each game keeps its
// view until the app closes.
const lastViews = new Map<number, { text: string; sort: MyModsSort }>();

/** My mods tab: search, enable, disable, remove with "Undo", one mod or a selection, share the list with friends. */
export function MyMods({ game, onDiscover }: { game: SteamGame; onDiscover: () => void }) {
	const appId = game.appId;
	const mods = useInstalled(appId);
	const updates = useUpdates(appId) ?? [];
	const saved = lastViews.get(appId);
	const [text, setText] = useState(saved?.text ?? "");
	const [sort, setSort] = useState<MyModsSort>(saved?.sort ?? "name");
	const [layout, setLayout] = useState(storedLayout);
	const [selecting, setSelecting] = useState(false);
	const [selected, setSelected] = useState<ReadonlySet<string>>(new Set());
	// Last mod picked: shift + click selects from there.
	const anchor = useRef<string | null>(null);
	const pendingOps = useGameStatus(appId)?.pending;

	useEffect(() => {
		lastViews.set(appId, { text, sort });
	}, [appId, text, sort]);

	if (mods === undefined) {
		return (
			<div className="max-w-5xl space-y-2">
				{[0, 1, 2].map((i) => (
					<Skeleton key={i} className="h-16 rounded-xl" />
				))}
			</div>
		);
	}

	if (mods.length === 0) {
		return (
			<div className="max-w-5xl">
				<InProgress appId={appId} />
				<div className="glass max-w-xl">
				<Notice
					icon={<Layers className="size-5 text-fg-muted" />}
					title="No mods installed yet"
					action={
						<div className="flex gap-2">
							<Button size="sm" onClick={onDiscover}>
								Discover mods
							</Button>
							{/* A friend's list: the quickest way to play together. */}
							<ImportButton game={game} mods={mods} variant="secondary" />
						</div>
					}
				>
					Mods you install show up here. Turn them on, off or remove them in one click.
				</Notice>
				</div>
			</div>
		);
	}

	const active = mods.filter((m) => m.enabled && !m.loaderRoot).length;
	const words = text.toLowerCase().split(/\s+/).filter(Boolean);
	// `mods` comes by name, loader first: "Name" has nothing to sort.
	const shown = mods.filter((m) => words.every((w) => `${m.name} ${m.author}`.toLowerCase().includes(w)));
	if (sort === "recent") shown.sort((a, b) => b.installedAt - a.installedAt);
	if (sort === "off") shown.sort((a, b) => Number(a.enabled) - Number(b.enabled));

	const compact = layout === "compact";
	// The loader follows the mods. A mod waiting to be removed keeps that plan.
	const selectable = (m: InstalledMod) =>
		m.loaderRoot === null && !pendingOps?.some((op) => op.type === "uninstall" && op.modId === m.id);
	const shownIds = shown.filter(selectable).map((m) => m.id);
	const picked = mods.filter((m) => selected.has(m.id) && selectable(m)).map((m) => m.id);
	const allShown = shownIds.length > 0 && shownIds.every((id) => selected.has(id));

	const chooseLayout = (value: MyModsLayout) => {
		setLayout(value);
		try {
			localStorage.setItem(LAYOUT_KEY, value);
		} catch {
			// Storage blocked: the list is back next time, no big deal.
		}
	};

	const stopSelecting = () => {
		setSelecting(false);
		setSelected(new Set());
		anchor.current = null;
	};

	const pick = (id: string, range: boolean) => {
		const from = shownIds.indexOf(anchor.current ?? id);
		const to = shownIds.indexOf(id);
		const ids = range && from >= 0 ? shownIds.slice(Math.min(from, to), Math.max(from, to) + 1) : [id];
		const add = !selected.has(id);
		const next = new Set(selected);
		for (const one of ids) {
			if (add) next.add(one);
			else next.delete(one);
		}
		anchor.current = id;
		setSelected(next);
	};

	const pickShown = () => {
		const next = new Set(selected);
		for (const id of shownIds) {
			if (allShown) next.delete(id);
			else next.add(id);
		}
		setSelected(next);
	};

	return (
		<div className={cn(!compact && "max-w-5xl")}>
			<InProgress appId={appId} />
			<div className="mb-3 flex items-center justify-between gap-4">
				<p className="text-xs text-fg-subtle">
					{active} active mod{active === 1 ? "" : "s"} · changes apply the next time you launch the game
				</p>
				{/* Outline: "Play" stays the only primary action of the screen. */}
				<div className="flex shrink-0 items-center gap-2">
					<UpdateCheck appId={appId} />
					{/* Always there: greyed out, it says everything is up to date. */}
					<Button
						size="sm"
						variant="outline"
						leftIcon={<ArrowUp className="size-3.5" />}
						disabled={updates.length === 0}
						title={updates.length === 0 ? "Every mod is up to date" : undefined}
						onClick={() => updateAll(appId, updates)}
					>
						{updates.length > 0 ? `Update all (${updates.length})` : "Update all"}
					</Button>
					<ImportButton game={game} mods={mods} />
					<ShareButton game={game} mods={mods} />
				</div>
			</div>
			<div className="mb-3 flex items-center gap-3">
				<SearchField
					value={text}
					onChange={(e) => setText(e.target.value)}
					placeholder="Search my mods"
					aria-label="Search my mods"
					className="h-10 max-w-sm flex-1"
				/>
				<Segmented label="Sort my mods" items={SORTS} value={sort} onChange={setSort} />
				<Segmented label="Layout of my mods" items={LAYOUTS} value={layout} onChange={chooseLayout} />
				<Button
					size="sm"
					variant="outline"
					leftIcon={<SquareCheck className="size-3.5" />}
					aria-pressed={selecting}
					className={cn("h-10", selecting && "bg-surface-strong")}
					onClick={() => (selecting ? stopSelecting() : setSelecting(true))}
				>
					Select
				</Button>
			</div>
			{shown.length === 0 ? (
				<div className="glass max-w-xl">
					<Notice icon={<PackageSearch className="size-5 text-fg-muted" />} title="No mods found">
						{`No installed mod matches “${text.trim()}”.`}
					</Notice>
				</div>
			) : (
				// No overflow-hidden: a row's version menu overflows onto the next ones.
				<ul className={compact ? "grid grid-cols-[repeat(auto-fill,minmax(20rem,1fr))] gap-2" : "rounded-xl border border-line bg-surface"}>
					{shown.map((m, i) => (
						<li key={m.id} className={compact ? "rounded-xl border border-line bg-surface" : cn(i > 0 && "border-t border-line")}>
							<ModRow
								mod={m}
								appId={appId}
								nexusDomain={game.profile?.nexus?.domain ?? null}
								compact={compact}
								selection={
									selecting
										? { checked: selected.has(m.id), disabled: !selectable(m), onPick: (range) => pick(m.id, range) }
										: undefined
								}
							/>
							<ModNeeds mod={m} appId={appId} className={compact ? "px-3 pb-2" : "pr-4 pb-3 pl-[68px]"} />
						</li>
					))}
				</ul>
			)}
			{selecting && (
				<SelectionBar appId={appId} picked={picked} allShown={allShown} onPickShown={pickShown} onClose={stopSelecting} />
			)}
		</div>
	);
}

/** What to do with the selected mods. In the middle, and above the beta bubble in a narrow window. */
function SelectionBar({
	appId,
	picked,
	allShown,
	onPickShown,
	onClose,
}: {
	appId: number;
	picked: string[];
	allShown: boolean;
	onPickShown: () => void;
	onClose: () => void;
}) {
	const [busy, setBusy] = useState(false);
	const inactive = busy || picked.length === 0;

	const run = async (action: () => Promise<void>) => {
		setBusy(true);
		await action().catch(fail);
		setBusy(false);
		onClose();
	};

	return (
		<div className="sticky bottom-6 z-[60] mx-auto mt-3 flex w-fit items-center gap-2 rounded-xl border border-line-strong bg-bg p-2 pl-4 shadow-2xl shadow-black/50">
			<span className="mr-1 text-xs text-fg-muted">{picked.length} selected</span>
			<Button size="sm" variant="secondary" disabled={busy} onClick={onPickShown}>
				{allShown ? "Clear" : "Select all"}
			</Button>
			<Button size="sm" variant="outline" disabled={inactive} onClick={() => void run(() => toggleMany(appId, picked, true))}>
				Turn on
			</Button>
			<Button size="sm" variant="outline" disabled={inactive} onClick={() => void run(() => toggleMany(appId, picked, false))}>
				Turn off
			</Button>
			<Button
				size="sm"
				variant="danger"
				leftIcon={<Trash2 className="size-3.5" />}
				disabled={inactive}
				onClick={() => void run(() => uninstallManyWithUndo(appId, picked))}
			>
				Remove
			</Button>
			<button
				type="button"
				aria-label="Stop selecting"
				title="Cancel"
				disabled={busy}
				onClick={onClose}
				className="cursor-pointer rounded-md p-2 text-fg-subtle transition-colors hover:bg-white/10 hover:text-fg"
			>
				<X className="size-4" />
			</button>
		</div>
	);
}

/** When the mods were last checked, and a way to check them again. */
function UpdateCheck({ appId }: { appId: number }) {
	const check = useUpdateCheck(appId);
	const [now, setNow] = useState(Date.now);

	// The label follows the clock while the tab stays open.
	useEffect(() => {
		const timer = window.setInterval(() => setNow(Date.now()), 30_000);
		return () => window.clearInterval(timer);
	}, []);

	const checked = check?.at ? `Checked ${relativeDate(new Date(check.at).toISOString(), Math.max(now, check.at))}` : "Check for updates";
	return (
		<button
			type="button"
			disabled={check?.running}
			title="Check for updates"
			onClick={() => runUpdateCheck(appId)}
			className="inline-flex cursor-pointer items-center gap-1.5 rounded-md px-2 py-1.5 text-xs text-fg-subtle transition-colors hover:bg-white/10 hover:text-fg disabled:pointer-events-none"
		>
			<RotateCw className={cn("size-3.5", check?.running && "animate-spin")} />
			{check?.running ? "Checking…" : check?.failed ? "Couldn't check, try again" : checked}
		</button>
	);
}

/**
 * The game's downloads and installs in progress, each cancelable, then the
 * paused ones (app closed): to resume where they were.
 */
function InProgress({ appId }: { appId: number }) {
	const active = useActiveInstalls(appId);
	const [paused, setPaused] = useState<PausedDownload[]>([]);
	const refresh = useCallback(() => {
		pausedDownloads(appId).then(setPaused, () => setPaused([]));
	}, [appId]);
	// An install that starts or ends changes the list of paused ones.
	useEffect(refresh, [refresh, active.length]);

	const waiting = paused.filter((p) => !active.some(([id]) => id === p.modId));
	const count = active.length + waiting.length;
	if (count === 0) return null;
	return (
		<section className="mb-6">
			<h2 className="mb-2 px-1 text-xs font-semibold tracking-wide text-fg-subtle uppercase">In progress ({count})</h2>
			<ul className="overflow-hidden rounded-xl border border-line bg-surface">
				{active.map(([modId, a], i) => (
					<li key={modId} className={cn(i > 0 && "border-t border-line")}>
						<ProgressRow appId={appId} modId={modId} active={a} />
					</li>
				))}
				{waiting.map((p, i) => (
					<li key={p.modId} className={cn((active.length > 0 || i > 0) && "border-t border-line")}>
						<PausedRow appId={appId} download={p} onDiscarded={refresh} />
					</li>
				))}
			</ul>
		</section>
	);
}

function PausedRow({ appId, download: p, onDiscarded }: { appId: number; download: PausedDownload; onDiscarded: () => void }) {
	const value = p.total ? p.received / p.total : 0;
	const discard = () =>
		discardDownload(appId, p.modId).then(onDiscarded, (error: unknown) => {
			const { message } = splitError(error);
			toast({ title: message, tone: "error" });
		});

	return (
		<div className="flex animate-row-in items-center gap-3 px-4 py-3">
			<div className="flex min-w-0 flex-1 flex-col gap-1.5">
				<span className="flex items-baseline justify-between gap-3">
					<span className="flex min-w-0 items-center gap-1.5">
						<Pause className="size-3.5 shrink-0 text-fg-subtle" />
						<span className="truncate text-sm font-medium text-fg">{p.name}</span>
					</span>
					<span className="shrink-0 font-tight text-[11px] text-fg-subtle">
						Paused{p.total ? ` · ${Math.round(value * 100)}%` : ""}
					</span>
				</span>
				<span className="relative h-1.5 overflow-hidden rounded-full bg-surface-strong">
					<span
						className="absolute inset-y-0 left-0 w-full origin-left rounded-full bg-fg-faint"
						style={{ scale: `${Math.max(value, 0.02)} 1` }}
					/>
				</span>
				<span className="truncate text-[11px] text-fg-subtle">
					{p.total ? `${bytes(p.received)} of ${bytes(p.total)} · ` : ""}picks up where it stopped
				</span>
			</div>
			<Button size="sm" variant="outline" leftIcon={<Play className="size-3.5" />} onClick={() => void install(appId, p.modId, p.name)}>
				Resume
			</Button>
			<button
				type="button"
				aria-label={`Discard ${p.name}`}
				title="Discard what was downloaded"
				onClick={() => void discard()}
				className="cursor-pointer rounded-md p-2 text-fg-subtle transition-colors hover:bg-danger/10 hover:text-danger"
			>
				<X className="size-4" />
			</button>
		</div>
	);
}

function ProgressRow({ appId, modId, active: a }: { appId: number; modId: string; active: ActiveInstall }) {
	const router = useRouter();
	const progress = a.status === "running" ? a.progress : null;
	// "resolving" doesn't have a package name yet.
	const name = a.name ?? (progress && "name" in progress ? progress.name : modId);
	const value = progress ? overallProgress(progress) : 0;

	return (
		<div className="flex animate-row-in items-center gap-3 px-4 py-3">
			<button
				type="button"
				onClick={() => router.push(modHref(appId, modId))}
				className="flex min-w-0 flex-1 cursor-pointer flex-col gap-1.5 text-left"
			>
				<span className="flex items-baseline justify-between gap-3">
					<span className="truncate text-sm font-medium text-fg">{name}</span>
					<span className="shrink-0 font-tight text-[11px] text-fg-subtle">
						{a.status === "website" ? "Waiting for Nexus Mods" : progressLabel(progress, value)}
					</span>
				</span>
				{a.status === "website" ? (
					<span className="text-[11px] text-fg-subtle">
						Click <span className="text-fg-muted">Slow download</span> on the Nexus Mods page. Unalia installs it right after.
					</span>
				) : (
					<>
						<span className="relative h-1.5 overflow-hidden rounded-full bg-surface-strong">
							<span
								className="absolute inset-y-0 left-0 w-full origin-left rounded-full bg-accent transition-[scale] duration-300 ease-out"
								style={{ scale: `${Math.max(value, 0.02)} 1` }}
							/>
						</span>
						<span className="truncate text-[11px] text-fg-subtle">
							{progressDetail(progress, a.status === "running" ? a.speed : undefined).replace("\n", " · ")}
						</span>
					</>
				)}
			</button>
			{a.status === "website" && (
				<>
					<Button size="sm" variant="outline" leftIcon={<ExternalLink className="size-3.5" />} onClick={() => void openExternal(a.url)}>
						Open page
					</Button>
					<button
						type="button"
						aria-label={`Copy the link of ${name}`}
						title="Copy link"
						onClick={() => void copyLink(a.url)}
						className="cursor-pointer rounded-md p-2 text-fg-subtle transition-colors hover:bg-white/10 hover:text-fg"
					>
						<Copy className="size-4" />
					</button>
				</>
			)}
			<button
				type="button"
				aria-label={`Cancel ${name}`}
				title="Cancel"
				onClick={() => void cancel(appId, modId)}
				className="cursor-pointer rounded-md p-2 text-fg-subtle transition-colors hover:bg-white/10 hover:text-fg"
			>
				<X className="size-4" />
			</button>
		</div>
	);
}

function ModRow({
	mod: m,
	appId,
	nexusDomain,
	compact,
	selection,
}: {
	mod: InstalledMod;
	appId: number;
	nexusDomain: string | null;
	/** Narrow row for a column: what doesn't fit stays in the list layout. */
	compact: boolean;
	/** Selection mode: a click picks the mod instead of opening it, its controls step aside. */
	selection?: { checked: boolean; disabled: boolean; onPick: (range: boolean) => void };
}) {
	const router = useRouter();
	const [busy, setBusy] = useState(false);
	const loader = m.loaderRoot !== null;
	// Change requested during a session: show the planned state.
	const pending = usePendingOp(appId, m.id);
	const removing = pending?.type === "uninstall";
	const enabled = pending?.type === "setEnabled" ? pending.enabled : m.enabled;
	const update = useModUpdate(appId, m.id);
	const deprecated = useDeprecated(appId, m.id);
	const active = useActiveInstall(appId, m.id);
	const updating = active?.status === "queued" || active?.status === "running";
	// Previous versions kept: the version becomes a menu to go back.
	const picker = m.previousVersions.length > 0 && !pending && !updating;
	const [menuOpen, setMenuOpen] = useState(false);
	const byline = `${m.author ? `by ${m.author}` : "\u00a0"}${
		m.coveredBy.length > 0 ? ` \u00b7 Some files replaced by ${m.coveredBy.join(", ")}` : ""
	}`;

	const onToggle = (value: boolean) => {
		toggle(appId, m.id, value).catch(fail);
	};

	const onRemove = async () => {
		setBusy(true);
		await uninstallWithUndo(appId, m.id, m.name);
		setBusy(false);
	};

	const badges = (
		<>
			{loader && <Badge tone="accent">Loader</Badge>}
			{!loader && !m.explicit && <Badge>Dependency</Badge>}
			{deprecated && (
				<Badge tone="danger" title="Its author retired this mod. See its page for what to use instead.">
					Deprecated
				</Badge>
			)}
			{pending && <Badge tone="accent">Pending</Badge>}
		</>
	);

	return (
		<div
			className={cn(
				"flex animate-row-in items-center",
				compact ? "gap-2 px-3 py-2" : "gap-3 px-4 py-3",
				(removing || selection?.disabled) && "opacity-60",
				// Version menu open: painted above the next rows.
				menuOpen && "relative z-20",
			)}
		>
			<button
				type="button"
				disabled={selection?.disabled}
				aria-pressed={selection?.checked}
				title={compact ? byline : undefined}
				onClick={(e) => (selection ? selection.onPick(e.shiftKey) : router.push(modHref(appId, m.id, m.source)))}
				className="flex min-w-0 flex-1 cursor-pointer items-center gap-3 text-left disabled:cursor-default"
			>
				{selection && <Tick checked={selection.checked} />}
				<ModThumb src={m.iconUrl} adult={m.adult} dimmed={!enabled || removing} />
				<span className="min-w-0">
					<span className="flex items-center gap-2">
						<Marquee text={m.name} className={cn("text-sm font-medium", enabled && !removing ? "text-fg" : "text-fg-subtle")} />
						{!compact && !picker && <span className="shrink-0 font-mono text-[11px] text-fg-muted">{versionLabel(m.version)}</span>}
						{!compact && badges}
						{!compact && update && !pending && <Badge tone="accent">{versionLabel(update.latest)} available</Badge>}
					</span>
					{compact ? (
						<span className="flex h-5 items-center gap-1.5 overflow-hidden">
							<span className="max-w-24 shrink-0 truncate font-mono text-[11px] text-fg-muted">{versionLabel(m.version)}</span>
							{badges}
							<span className="min-w-0 truncate text-xs text-fg-subtle">{m.author ? `by ${m.author}` : ""}</span>
						</span>
					) : (
						<Marquee className="text-xs text-fg-subtle" text={byline} />
					)}
				</span>
			</button>

			{!selection && (
				<>
					{picker && !compact && <VersionPicker mod={m} appId={appId} onError={fail} onOpenChange={setMenuOpen} />}
					{updating ? (
						<span className="flex items-center gap-1.5 text-[11px] text-accent">
							<Spinner className="size-3" />
							{!compact && "Updating…"}
						</span>
					) : (
						update &&
						!pending &&
						(compact ? (
							<button
								type="button"
								aria-label={`Update ${m.name}`}
								title={`Update to ${versionLabel(update.latest)}`}
								onClick={() => void install(appId, m.id, m.name)}
								className="cursor-pointer rounded-md p-1.5 text-accent transition-colors hover:bg-white/10"
							>
								<ArrowUp className="size-4" />
							</button>
						) : (
							<Button size="sm" variant="outline" onClick={() => void install(appId, m.id, m.name)}>
								Update
							</Button>
						))
					)}
					{removing ? (
						!compact && <span className="text-[11px] text-fg-subtle">Removed when you close the game</span>
					) : loader ? (
						!compact && <span className="text-[11px] text-fg-faint">Required by your mods</span>
					) : (
						<Switch checked={enabled} onChange={onToggle} label={`${enabled ? "Turn off" : "Turn on"} ${m.name}`} />
					)}
					{!compact && nexusDomain && /^nexus-\d+$/.test(m.id) && (
						<button
							type="button"
							aria-label={`View ${m.name} on Nexus Mods`}
							title="View on Nexus Mods"
							onClick={() => void openExternal(nexusModUrl(nexusDomain, m.id))}
							className="cursor-pointer rounded-md p-2 text-fg-subtle transition-colors hover:bg-white/10 hover:text-fg"
						>
							<ExternalLink className="size-4" />
						</button>
					)}
					<button
						type="button"
						aria-label={`Remove ${m.name}`}
						title="Remove"
						disabled={busy || removing}
						onClick={onRemove}
						className={cn(
							"cursor-pointer rounded-md text-fg-subtle transition-colors hover:bg-danger/10 hover:text-danger disabled:opacity-50",
							compact ? "p-1.5" : "p-2",
						)}
					>
						<Trash2 className="size-4" />
					</button>
				</>
			)}
		</div>
	);
}

function Tick({ checked }: { checked: boolean }) {
	return (
		<span
			className={cn(
				"flex size-4 shrink-0 items-center justify-center rounded border",
				checked ? "border-transparent bg-accent text-bg" : "border-line-strong bg-surface-strong",
			)}
		>
			{checked && <Check className="size-3" strokeWidth={3} />}
		</span>
	);
}

/**
 * Installed version, and the previous ones to go back in one click: they
 * are kept in the storage, nothing is downloaded again.
 */
function VersionPicker({
	mod: m,
	appId,
	onError,
	onOpenChange,
}: {
	mod: InstalledMod;
	appId: number;
	onError: (error: unknown) => void;
	onOpenChange: (open: boolean) => void;
}) {
	const [busy, setBusy] = useState(false);
	const choose = (pkg: string) => {
		if (pkg === m.package) return;
		setBusy(true);
		revert(appId, m.id, pkg)
			.then((reverted) => {
				toast({
					title: `${m.name} is back to ${versionLabel(reverted.version)}`,
					details: "Undo returns to the version you left.",
					// The version left became a previous version.
					action: { label: "Undo", onClick: () => void revert(appId, m.id, m.package).catch(onError) },
				});
			}, onError)
			.finally(() => setBusy(false));
	};

	return (
		// Long versions (`v3.0.1-1183-gcdbc0b66-r2`) are cut: the tooltip has them whole.
		<span title={`${versionLabel(m.version)}: go back to a version you had before, without downloading it again`}>
			<Select value={m.package} onValueChange={choose} onOpenChange={onOpenChange} disabled={busy} className="w-28">
				<SelectTrigger className="h-7 px-2.5 py-0 font-mono text-[11px]">
					<SelectValue className="min-w-0 truncate" />
				</SelectTrigger>
				<SelectContent>
					<SelectItem value={m.package} className="truncate font-mono text-xs">
						{versionLabel(m.version)}
					</SelectItem>
					{m.previousVersions.map((v) => (
						<SelectItem key={v.package} value={v.package} className="truncate font-mono text-xs">
							{versionLabel(v.version)}
						</SelectItem>
					))}
				</SelectContent>
			</Select>
		</span>
	);
}

/**
 * Readable version: `v1.4`, but a date as is (old Nexus file without a
 * number, installed for an older version of the game).
 */
function versionLabel(version: string): string {
	if (!version) return "";
	return /^\d{4}-\d{2}-\d{2}$/.test(version) ? version : `v${version}`;
}

function ModThumb({ src, adult, dimmed }: { src: string | null; adult: boolean; dimmed: boolean }) {
	const [failed, setFailed] = useState(false);
	const blur = useAdultBlur(adult);
	return (
		<span
			className={cn(
				"flex size-10 shrink-0 items-center justify-center overflow-hidden rounded-lg border border-line bg-white/[0.04]",
				dimmed && "opacity-50 grayscale",
			)}
		>
			{src && !failed ? (
				<img src={src} alt="" style={blur} className="size-full object-cover" onError={() => setFailed(true)} />
			) : (
				<Package className="size-4 text-fg-faint" />
			)}
		</span>
	);
}
