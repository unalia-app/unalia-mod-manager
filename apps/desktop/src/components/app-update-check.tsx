"use client";

import { Button, Dialog, DialogDescription, DialogFooter, DialogHeader, DialogTitle, cn } from "@modalia/ui";
import { Download } from "lucide-react";
import { useEffect, useState } from "react";

import { ReleaseNotesList } from "@/components/app-update/release-notes";
import { type AppUpdateState, checkForUpdate, installUpdate, previewUpdate, useAppUpdate } from "@/lib/app-update";

/** Lets the home screen load before asking the release server. */
const CHECK_AFTER_MS = 5_000;

/**
 * At startup, a new app version announces itself in a dialog, with its
 * notes: a toast went unnoticed. "Later" closes it until the next launch
 * (Settings > Support always offers it). No error if the check fails
 * (offline): Settings > Support lets you try again.
 */
export function AppUpdateCheck() {
	const update = useAppUpdate();
	const [open, setOpen] = useState(false);

	useEffect(() => {
		if (process.env.NODE_ENV === "development") {
			// Dev preview: Ctrl+Shift+U (`DevShortcuts`).
			const preview = () => {
				previewUpdate();
				setOpen(true);
			};
			window.addEventListener("unalia:preview-update", preview);
			return () => window.removeEventListener("unalia:preview-update", preview);
		}
		const timer = window.setTimeout(() => {
			checkForUpdate().then(
				(found) => found && setOpen(true),
				() => {},
			);
		}, CHECK_AFTER_MS);
		return () => window.clearTimeout(timer);
	}, []);

	const target = "update" in update ? update.update : null;
	if (!target) return null;
	const installing = update.status === "installing";

	return (
		<Dialog open={open} onClose={installing ? undefined : () => setOpen(false)} className="grain max-w-lg bg-[#08080a]">
			<Glow className="from-success/15" />
			<DialogHeader>
				<div className="flex items-center gap-3">
					<span className="flex size-9 shrink-0 items-center justify-center rounded-xl border border-success/20 bg-success/10">
						<Download className="size-4 text-success" />
					</span>
					<DialogTitle>A new version is available</DialogTitle>
					<span className="rounded-full border border-success/20 bg-success/10 px-2 py-0.5 font-mono text-[11px] text-success">
						v{target.version}
					</span>
				</div>
				<DialogDescription>The app restarts on the new version. Your mods stay as they are.</DialogDescription>
			</DialogHeader>

			{target.notes && (
				<ReleaseNotesList notes={target.notes} className="-mx-6 max-h-[45vh] border-y border-line px-6 py-4" />
			)}

			{installing && <Progress value={update.progress} />}

			{update.status === "error" && (
				<div className="text-xs">
					<p className="text-danger">{update.message}</p>
					{update.details && (
						<details className="mt-1 text-fg-faint">
							<summary className="cursor-pointer select-none hover:text-fg-subtle">Technical details</summary>
							<p className="mt-1 font-mono break-words text-fg-subtle">{update.details}</p>
						</details>
					)}
				</div>
			)}

			<DialogFooter>
				<Button variant="secondary" disabled={installing} onClick={() => setOpen(false)}>
					Later
				</Button>
				<Button
					leftIcon={<Download className="size-3.5" />}
					isLoading={installing}
					onClick={() => void installUpdate().catch(() => {})}
				>
					{update.status === "error" ? "Try again" : "Update now"}
				</Button>
			</DialogFooter>
		</Dialog>
	);
}

/** Glow of the dialog color at the top, over the grain. */
export function Glow({ className }: { className: string }) {
	return (
		<div
			aria-hidden
			className={cn(
				"pointer-events-none absolute inset-x-0 top-0 -z-10 h-48 rounded-t-2xl bg-radial-[ellipse_at_50%_0%] to-transparent to-70%",
				className,
			)}
		/>
	);
}

/** Download progress; unknown size: animated bar. */
function Progress({ value }: { value: Extract<AppUpdateState, { status: "installing" }>["progress"] }) {
	return (
		<div className="flex flex-col gap-2">
			<div
				role="progressbar"
				aria-valuemin={0}
				aria-valuemax={100}
				aria-valuenow={value === null ? undefined : Math.round(value * 100)}
				aria-label="Downloading the update"
				className="relative h-1.5 overflow-hidden rounded-full bg-surface-strong"
			>
				{value === null ? (
					<div className="absolute inset-y-0 w-1/3 animate-shimmer bg-gradient-to-r from-transparent via-success/70 to-transparent" />
				) : (
					<div
						className="absolute inset-y-0 left-0 w-full origin-left bg-success transition-[scale] duration-300 ease-out"
						style={{ scale: `${Math.max(value, 0.02)} 1` }}
					/>
				)}
			</div>
			<p className="text-xs text-fg-subtle">
				{value === null ? "Downloading…" : `Downloading… ${Math.round(value * 100)}%`} The app closes and reopens on its
				own.
			</p>
		</div>
	);
}
