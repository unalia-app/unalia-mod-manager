"use client";

import { Button, cn } from "@modalia/ui";
import { ArrowUp, Check, Clock, Download, ExternalLink, RotateCw, Trash2, X } from "lucide-react";
import { useState } from "react";

import { uninstallWithUndo } from "@/components/mod/uninstall";
import { type InstallProgress, cancelPending } from "@/lib/engine";
import { bytes, timeLeft } from "@/lib/format";
import { usePendingOp } from "@/lib/game-status";
import { cancel, clearError, install, overallProgress, useActiveInstall, useInstalled, useModUpdate } from "@/lib/installs";
import { openExternal } from "@/lib/open";

/**
 * One click: the button itself becomes the progress bar (download, then
 * install), then "Installed". Never a popup.
 */
export function InstallButton({
	appId,
	modId,
	modName,
	dependencyCount,
	hint,
}: {
	appId: number;
	modId: string;
	modName: string;
	dependencyCount: number;
	/** Detail under the button (e.g. confirmation on Nexus for a free account). */
	hint?: string;
}) {
	const installed = useInstalled(appId)?.find((m) => m.id === modId);
	const active = useActiveInstall(appId, modId);
	const pending = usePendingOp(appId, modId);
	const update = useModUpdate(appId, modId);

	// The result (success or error) arrives through the store, which the button follows.
	const start = () => void install(appId, modId, modName);

	if (active?.status === "queued" || active?.status === "running") {
		const progress = active.status === "running" ? active.progress : null;
		const speed = active.status === "running" ? active.speed : undefined;
		const value = progress ? overallProgress(progress) : 0;
		return (
			<div className="flex flex-col items-end gap-2">
				<div
					role="progressbar"
					aria-valuemin={0}
					aria-valuemax={100}
					aria-valuenow={Math.round(value * 100)}
					aria-label={`Installing ${modName}`}
					className="relative h-10 w-48 overflow-hidden rounded-md border border-line-strong bg-surface-strong"
				>
					<div
						className="absolute inset-y-0 left-0 w-full origin-left bg-accent/35 transition-[scale] duration-300 ease-out"
						style={{ scale: `${Math.max(value, 0.02)} 1` }}
					/>
					<div className="absolute inset-y-0 w-1/3 animate-shimmer bg-gradient-to-r from-transparent via-white/10 to-transparent" />
					<span className="relative flex h-full items-center justify-center gap-2 px-3 text-xs font-medium text-fg">
						{progressLabel(progress, value)}
					</span>
				</div>
				<span className="text-right text-[11px] whitespace-pre-line text-fg-subtle">{progressDetail(progress, speed)}</span>
				<button
					type="button"
					onClick={() => void cancel(appId, modId)}
					className="flex cursor-pointer items-center gap-1 text-[11px] text-fg-muted underline-offset-2 hover:text-fg hover:underline"
				>
					<X className="size-3" />
					Cancel
				</button>
			</div>
		);
	}

	// Free Nexus account: Nexus wants a click on its site. The `nxm://`
	// link comes back to Modalia, which carries on by itself.
	if (active?.status === "website") {
		return (
			<div className="flex max-w-64 flex-col items-end gap-2 text-right">
				<Button
					size="lg"
					variant="outline"
					leftIcon={<ExternalLink className="size-4 text-accent" />}
					onClick={() => void openExternal(active.url)}
				>
					Finish on Nexus Mods
				</Button>
				<span className="text-[11px] leading-snug text-fg-subtle">
					Click <span className="text-fg-muted">Slow download</span> on the page that opened. Unalia installs it right after ·{" "}
					<button
						type="button"
						className="cursor-pointer text-fg-muted underline-offset-2 hover:underline"
						onClick={() => clearError(appId, modId)}
					>
						Cancel
					</button>
				</span>
			</div>
		);
	}

	// The game runs: the install waits for it to close.
	if (pending?.type === "install") {
		return (
			<div className="flex flex-col items-end gap-2">
				<Button size="lg" variant="outline" disabled leftIcon={<Clock className="size-4 text-accent" />} className="disabled:opacity-100">
					Queued
				</Button>
				<span className="text-[11px] text-fg-subtle">
					{pending.ready ? "Downloaded · installs when you close the game" : "Downloading in the background"} ·{" "}
					<button
						type="button"
						className="cursor-pointer text-fg-muted underline-offset-2 hover:underline"
						onClick={() => void cancelPending(appId, modId)}
					>
						Cancel
					</button>
				</span>
			</div>
		);
	}

	// No automatic update: we offer, we don't impose.
	if (installed && update && pending?.type !== "uninstall") {
		return (
			<div className="flex flex-col items-end gap-2">
				<Button size="lg" leftIcon={<ArrowUp className="size-4" />} onClick={start}>
					{`Update to v${update.latest}`}
				</Button>
				<span className="text-[11px] text-fg-subtle">You have v{update.current}</span>
			</div>
		);
	}

	// The game runs: the removal waits for it to close.
	if (pending?.type === "uninstall") {
		return (
			<div className="flex flex-col items-end gap-2">
				<Button size="lg" variant="outline" disabled leftIcon={<Clock className="size-4 text-danger" />} className="disabled:opacity-100">
					Uninstall queued
				</Button>
				<span className="text-[11px] text-fg-subtle">
					Removed when you close the game ·{" "}
					<button
						type="button"
						className="cursor-pointer text-fg-muted underline-offset-2 hover:underline"
						onClick={() => void cancelPending(appId, modId)}
					>
						Cancel
					</button>
				</span>
			</div>
		);
	}

	if (installed) {
		return <InstalledButton appId={appId} modId={modId} modName={modName} version={installed.version} enabled={installed.enabled} />;
	}

	if (active?.status === "error") {
		return (
			<div className="flex max-w-64 flex-col items-end gap-2 text-right">
				<Button
					size="lg"
					leftIcon={<RotateCw className="size-4" />}
					onClick={() => {
						clearError(appId, modId);
						start();
					}}
				>
					Try again
				</Button>
				<span className="text-[11px] leading-snug text-danger">{active.message}</span>
				{active.details && (
					<details className="text-[11px] text-fg-faint">
						<summary className="cursor-pointer select-none hover:text-fg-subtle">Technical details</summary>
						<p className="mt-1 font-mono break-words text-fg-subtle">{active.details}</p>
					</details>
				)}
			</div>
		);
	}

	return (
		<div className="flex flex-col items-end gap-2">
			<Button size="lg" leftIcon={<Download className="size-4" />} onClick={start}>
				Install
			</Button>
			{dependencyCount > 0 && (
				<span className="text-[11px] text-fg-subtle">
					+ {dependencyCount} {dependencyCount > 1 ? "dependencies" : "dependency"}
				</span>
			)}
			{hint && <span className="max-w-64 text-right text-[11px] leading-snug text-fg-faint">{hint}</span>}
		</div>
	);
}

/** "Installed", which becomes "Uninstall" on hover (removal with Undo). */
function InstalledButton({
	appId,
	modId,
	modName,
	version,
	enabled,
}: {
	appId: number;
	modId: string;
	modName: string;
	version: string;
	enabled: boolean;
}) {
	const [removing, setRemoving] = useState(false);

	const uninstall = async () => {
		setRemoving(true);
		await uninstallWithUndo(appId, modId, modName);
		setRemoving(false);
	};

	return (
		<div className="flex flex-col items-end gap-2">
			<Button
				size="lg"
				variant="outline"
				isLoading={removing}
				aria-label={`Uninstall ${modName}`}
				onClick={() => void uninstall()}
				className="group/installed hover:border-danger/30 hover:bg-danger/10 hover:text-danger focus-visible:border-danger/30 focus-visible:bg-danger/10 focus-visible:text-danger"
			>
				{/* Both labels in the same cell: fixed width, nothing moves on
				    hover. */}
				<span className="grid">
					<span className="col-start-1 row-start-1 flex items-center justify-center gap-2.5 group-hover/installed:invisible group-focus-visible/installed:invisible">
						<Check className="size-4 text-success" />
						Installed
					</span>
					<span className="invisible col-start-1 row-start-1 flex items-center justify-center gap-2.5 group-hover/installed:visible group-focus-visible/installed:visible">
						<Trash2 className="size-4" />
						Uninstall
					</span>
				</span>
			</Button>
			<span className="text-[11px] text-fg-subtle">
				v{version}
				{enabled ? "" : " · turned off"}
			</span>
		</div>
	);
}

/** The percentage is global, like the bar; the current package is below. */
export function progressLabel(progress: InstallProgress | null, value: number): string {
	if (!progress) return "Waiting…";
	if (progress.stage === "resolving") return "Preparing…";
	const percent = Math.round(value * 100);
	if (progress.stage === "downloading") return `Downloading ${percent}%`;
	if (progress.stage === "extracting") return `Unpacking ${percent}%`;
	return `Installing ${percent}%`;
}

/**
 * Current package, and for big files the amount, the speed and the time
 * left: "3.2 GB of 10 GB · 24 MB/s · 5 min left".
 */
export function progressDetail(progress: InstallProgress | null, speed?: number): string {
	if (!progress || progress.stage === "resolving") return "Checking dependencies";
	const which = progress.total > 1 ? `${progress.name} · ${progress.index + 1} of ${progress.total}` : progress.name;
	if (progress.stage === "downloading" && progress.size && progress.size > 20 * 1024 * 1024) {
		const parts = [`${bytes(progress.received)} of ${bytes(progress.size)}`];
		if (speed && speed > 0) {
			parts.push(`${bytes(speed)}/s`, timeLeft((progress.size - progress.received) / speed));
		}
		return `${which}\n${parts.filter(Boolean).join(" · ")}`;
	}
	if (progress.stage === "extracting" && progress.size > 20 * 1024 * 1024) {
		return `${which}\nUnpacking ${bytes(progress.done)} of ${bytes(progress.size)}`;
	}
	return which;
}

/** Card badge: installed / in progress. */
export function InstallStateBadge({ appId, modId, className }: { appId: number; modId: string; className?: string }) {
	const installed = useInstalled(appId)?.some((m) => m.id === modId);
	const active = useActiveInstall(appId, modId);
	const pending = usePendingOp(appId, modId)?.type === "install";
	const update = useModUpdate(appId, modId);
	const running = active?.status === "queued" || active?.status === "running";
	if (!installed && !running && !pending) return null;
	const accent = running || pending || Boolean(update);
	return (
		<span
			className={cn(
				"liquid-glass inline-flex items-center gap-1 rounded-full px-2 py-0.5 font-tight text-[11px] font-semibold tracking-wide",
				accent ? "text-accent" : "text-success",
				className,
			)}
		>
			{pending ? "Queued" : running ? "Installing…" : update ? (
				<>
					<ArrowUp className="size-3" />
					Update
				</>
			) : (
				<>
					<Check className="size-3" />
					Installed
				</>
			)}
		</span>
	);
}
