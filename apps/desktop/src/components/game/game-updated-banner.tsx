"use client";

import { Button } from "@modalia/ui";
import { RefreshCw, TriangleAlert, X } from "lucide-react";
import { useState } from "react";

import { toast } from "@/components/toaster";
import { type SteamGame, splitError } from "@/lib/engine";
import { relativeDate } from "@/lib/format";
import { recheckUpdates, toggle, useInstalled, useUpdates } from "@/lib/installs";

const DISMISSED = "modalia.updateBannerDismissed";

function dismissedFor(appId: number): number | null {
	try {
		const all: unknown = JSON.parse(localStorage.getItem(DISMISSED) ?? "{}");
		const value = (all as Record<string, unknown>)[appId];
		return typeof value === "number" ? value : null;
	} catch {
		return null;
	}
}

function dismiss(appId: number, lastUpdated: number) {
	try {
		const all = JSON.parse(localStorage.getItem(DISMISSED) ?? "{}") as Record<string, number>;
		localStorage.setItem(DISMISSED, JSON.stringify({ ...all, [appId]: lastUpdated }));
	} catch {
		// Storage blocked: the banner comes back, no big deal.
	}
}

/**
 * The game was updated after some mods were installed: they may prevent it
 * from starting (scripts, frameworks). We say so, look for updates, and
 * offer to play without them.
 */
export function GameUpdatedBanner({ game }: { game: SteamGame }) {
	const mods = useInstalled(game.appId) ?? [];
	const updates = useUpdates(game.appId);
	const [hidden, setHidden] = useState(() => dismissedFor(game.appId) === game.lastUpdated);
	const [busy, setBusy] = useState(false);

	const updatedAt = game.lastUpdated;
	if (!updatedAt || hidden) return null;
	const older = mods.filter((m) => m.enabled && !m.loaderRoot && m.installedAt < updatedAt);
	if (older.length === 0) return null;

	const withUpdate = older.filter((m) => updates?.some((u) => u.modId === m.id)).length;
	const date = relativeDate(new Date(updatedAt * 1000).toISOString());

	const turnOff = async () => {
		setBusy(true);
		try {
			for (const m of older) await toggle(game.appId, m.id, false);
			toast({ title: `${older.length} mod${older.length > 1 ? "s" : ""} turned off`, details: "Turn them back on in My mods once they're updated." });
		} catch (error) {
			const { message, details } = splitError(error);
			toast({ title: message, details: details ?? undefined, tone: "error" });
		} finally {
			setBusy(false);
		}
	};

	return (
		<div className="mt-6 flex animate-row-in flex-wrap items-center gap-x-4 gap-y-3 rounded-xl border border-warning/25 bg-warning/[0.07] px-4 py-3">
			<span className="flex min-w-0 flex-1 items-start gap-2 text-sm text-fg">
				<TriangleAlert className="mt-0.5 size-4 shrink-0 text-warning" />
				<span>
					<span className="font-medium">
						{game.name} was updated {date}.
					</span>{" "}
					<span className="text-fg-muted">
						{older.length} mod{older.length > 1 ? "s were" : " was"} installed before
						{withUpdate > 0 ? `, ${withUpdate} already ${withUpdate > 1 ? "have" : "has"} an update` : ""}. If the game
						doesn't start, turn them off until their authors update them.
					</span>
				</span>
			</span>
			<div className="flex items-center gap-2">
				<Button
					size="sm"
					variant="outline"
					leftIcon={<RefreshCw className="size-3.5" />}
					onClick={() => recheckUpdates(game.appId)}
				>
					Check for updates
				</Button>
				<Button size="sm" variant="outline" disabled={busy} onClick={() => void turnOff()}>
					Turn them off
				</Button>
				<button
					type="button"
					aria-label="Hide this message"
					onClick={() => {
						dismiss(game.appId, updatedAt);
						setHidden(true);
					}}
					className="cursor-pointer rounded-md p-1.5 text-fg-faint transition-colors hover:bg-white/10 hover:text-fg"
				>
					<X className="size-3.5" />
				</button>
			</div>
		</div>
	);
}
