"use client";

import {
	Badge,
	Button,
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
import { ArrowUp, ExternalLink, Layers, Package, Pause, Play, Trash2, X } from "lucide-react";
import { useRouter } from "next/navigation";
import { useCallback, useEffect, useState } from "react";

import { progressDetail, progressLabel } from "@/components/mod/install-button";
import { uninstallWithUndo } from "@/components/mod/uninstall";
import { Notice } from "@/components/notice";
import { toast } from "@/components/toaster";
import { type InstalledMod, type PausedDownload, discardDownload, pausedDownloads, splitError } from "@/lib/engine";
import { bytes } from "@/lib/format";
import { usePendingOp } from "@/lib/game-status";
import { modHref } from "@/lib/games-store";
import { openExternal } from "@/lib/open";
import {
	type ActiveInstall,
	cancel,
	install,
	overallProgress,
	revert,
	toggle,
	updateAll,
	useActiveInstall,
	useActiveInstalls,
	useInstalled,
	useModUpdate,
	useUpdates,
} from "@/lib/installs";

/** My mods tab: enable, disable, remove with "Undo". */
export function MyMods({ appId, onDiscover }: { appId: number; onDiscover: () => void }) {
	const mods = useInstalled(appId);
	const updates = useUpdates(appId) ?? [];

	if (mods === undefined) {
		return (
			<div className="max-w-3xl space-y-2">
				{[0, 1, 2].map((i) => (
					<Skeleton key={i} className="h-16 rounded-xl" />
				))}
			</div>
		);
	}

	if (mods.length === 0) {
		return (
			<div className="max-w-3xl">
				<InProgress appId={appId} />
				<div className="glass max-w-xl">
				<Notice
					icon={<Layers className="size-5 text-fg-muted" />}
					title="No mods installed yet"
					action={
						<Button size="sm" onClick={onDiscover}>
							Discover mods
						</Button>
					}
				>
					Mods you install show up here. Turn them on, off or remove them in one click.
				</Notice>
				</div>
			</div>
		);
	}

	const active = mods.filter((m) => m.enabled && !m.loaderRoot).length;

	return (
		<div className="max-w-3xl">
			<InProgress appId={appId} />
			<div className="mb-3 flex items-center justify-between gap-4">
				<p className="text-xs text-fg-subtle">
					{active} active mod{active === 1 ? "" : "s"} · changes apply the next time you launch the game
				</p>
				{updates.length > 0 && (
					// Outline: "Play" stays the only primary action of the screen.
					<Button
						size="sm"
						variant="outline"
						leftIcon={<ArrowUp className="size-3.5" />}
						onClick={() => updateAll(appId, updates)}
					>
						{`Update all (${updates.length})`}
					</Button>
				)}
			</div>
			{/* No overflow-hidden: a row's version menu overflows onto the next ones. */}
			<ul className="rounded-xl border border-line bg-surface">
				{mods.map((m, i) => (
					<li key={m.id} className={cn(i > 0 && "border-t border-line")}>
						<ModRow mod={m} appId={appId} />
					</li>
				))}
			</ul>
		</div>
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
				<Button size="sm" variant="outline" leftIcon={<ExternalLink className="size-3.5" />} onClick={() => void openExternal(a.url)}>
					Open page
				</Button>
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

function ModRow({ mod: m, appId }: { mod: InstalledMod; appId: number }) {
	const router = useRouter();
	const [busy, setBusy] = useState(false);
	const loader = m.loaderRoot !== null;
	// Change requested during a session: show the planned state.
	const pending = usePendingOp(appId, m.id);
	const removing = pending?.type === "uninstall";
	const enabled = pending?.type === "setEnabled" ? pending.enabled : m.enabled;
	const update = useModUpdate(appId, m.id);
	const active = useActiveInstall(appId, m.id);
	const updating = active?.status === "queued" || active?.status === "running";
	// Previous versions kept: the version becomes a menu to go back.
	const picker = m.previousVersions.length > 0 && !pending && !updating;
	const [menuOpen, setMenuOpen] = useState(false);

	const fail = (error: unknown) => {
		const { message, details } = splitError(error);
		toast({ title: message, details: details ?? undefined, tone: "error" });
	};

	const onToggle = (value: boolean) => {
		toggle(appId, m.id, value).catch(fail);
	};

	const onRemove = async () => {
		setBusy(true);
		await uninstallWithUndo(appId, m.id, m.name);
		setBusy(false);
	};

	return (
		<div
			className={cn(
				"flex animate-row-in items-center gap-3 px-4 py-3",
				removing && "opacity-60",
				// Version menu open: painted above the next rows.
				menuOpen && "relative z-20",
			)}
		>
			<button
				type="button"
				onClick={() => router.push(modHref(appId, m.id, m.source))}
				className="flex min-w-0 flex-1 cursor-pointer items-center gap-3 text-left"
			>
				<ModThumb src={m.iconUrl} dimmed={!enabled || removing} />
				<span className="min-w-0">
					<span className="flex items-center gap-2">
						<span className={cn("truncate text-sm font-medium", enabled && !removing ? "text-fg" : "text-fg-subtle")}>
							{m.name}
						</span>
						{!picker && <span className="shrink-0 font-mono text-[11px] text-fg-muted">{versionLabel(m.version)}</span>}
						{loader && <Badge tone="accent">Loader</Badge>}
						{!loader && !m.explicit && <Badge>Dependency</Badge>}
						{pending && <Badge tone="accent">Pending</Badge>}
						{update && !pending && <Badge tone="accent">{versionLabel(update.latest)} available</Badge>}
					</span>
					<span className="block truncate text-xs text-fg-subtle">
						{m.author ? `by ${m.author}` : "\u00a0"}
					</span>
				</span>
			</button>

			{picker && <VersionPicker mod={m} appId={appId} onError={fail} onOpenChange={setMenuOpen} />}
			{updating ? (
				<span className="flex items-center gap-1.5 text-[11px] text-accent">
					<Spinner className="size-3" />
					Updating…
				</span>
			) : (
				update &&
				!pending && (
					<Button size="sm" variant="outline" onClick={() => void install(appId, m.id, m.name)}>
						Update
					</Button>
				)
			)}
			{removing ? (
				<span className="text-[11px] text-fg-subtle">Removed when you close the game</span>
			) : loader ? (
				<span className="text-[11px] text-fg-faint">Required by your mods</span>
			) : (
				<Switch checked={enabled} onChange={onToggle} label={`${enabled ? "Turn off" : "Turn on"} ${m.name}`} />
			)}
			<button
				type="button"
				aria-label={`Remove ${m.name}`}
				title="Remove"
				disabled={busy || removing}
				onClick={onRemove}
				className="cursor-pointer rounded-md p-2 text-fg-subtle transition-colors hover:bg-danger/10 hover:text-danger disabled:opacity-50"
			>
				<Trash2 className="size-4" />
			</button>
		</div>
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
					// The version left became a previous version.
					action: { label: "Undo", onClick: () => void revert(appId, m.id, m.package).catch(onError) },
				});
			}, onError)
			.finally(() => setBusy(false));
	};

	return (
		<span title="Go back to a version you had before, without downloading it again">
			<Select value={m.package} onValueChange={choose} onOpenChange={onOpenChange} disabled={busy} className="w-28">
				<SelectTrigger className="h-7 px-2.5 py-0 font-mono text-[11px]">
					<SelectValue />
				</SelectTrigger>
				<SelectContent>
					<SelectItem value={m.package} className="font-mono text-xs">
						{versionLabel(m.version)}
					</SelectItem>
					{m.previousVersions.map((v) => (
						<SelectItem key={v.package} value={v.package} className="font-mono text-xs">
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

function ModThumb({ src, dimmed }: { src: string | null; dimmed: boolean }) {
	const [failed, setFailed] = useState(false);
	return (
		<span
			className={cn(
				"flex size-10 shrink-0 items-center justify-center overflow-hidden rounded-lg border border-line bg-white/[0.04]",
				dimmed && "opacity-50 grayscale",
			)}
		>
			{src && !failed ? (
				<img src={src} alt="" className="size-full object-cover" onError={() => setFailed(true)} />
			) : (
				<Package className="size-4 text-fg-faint" />
			)}
		</span>
	);
}
