"use client";

import { Button, Spinner, Switch } from "@modalia/ui";
import { useState } from "react";

import { toast } from "@/components/toaster";
import { type SteamGame, addToSteam, setLaunchViaSteam, splitError } from "@/lib/engine";
import { patchGame } from "@/lib/games-store";

/**
 * Launch through Steam for a manually added game, to get the Steam overlay
 * (FPS counter, screenshots). Already added to Steam: a switch. Otherwise:
 * Unalia adds it, with its name and images.
 */
export function SteamLaunch({ game }: { game: SteamGame }) {
	const [shortcut, setShortcut] = useState(game.steamShortcut);
	const [enabled, setEnabled] = useState(game.launchViaSteam);
	const [confirming, setConfirming] = useState(false);
	const [adding, setAdding] = useState(false);
	if (game.source !== "manual") return null;

	const fail = (error: unknown) => {
		const { message, details } = splitError(error);
		toast({ title: message, details: details ?? undefined, tone: "error" });
	};

	const change = (value: boolean) => {
		setEnabled(value);
		setLaunchViaSteam(game.appId, value).then(
			() => patchGame(game.appId, { launchViaSteam: value }),
			(error: unknown) => {
				setEnabled(!value);
				fail(error);
			},
		);
	};

	const add = async () => {
		setAdding(true);
		try {
			await addToSteam(game.appId);
			setShortcut(true);
			setEnabled(true);
			await patchGame(game.appId, { steamShortcut: true, launchViaSteam: true });
			toast({ title: `${game.name} added to Steam`, details: "Play now starts it from Steam, with its overlay and FPS counter." });
		} catch (error) {
			fail(error);
		} finally {
			setAdding(false);
			setConfirming(false);
		}
	};

	if (shortcut) {
		return (
			<label
				className="mt-2.5 flex items-center justify-end gap-2 text-[11px] text-fg-subtle"
				title="Play starts the game from Steam, with its overlay and FPS counter"
			>
				Launch through Steam
				<Switch checked={enabled} onChange={change} label="Launch through Steam" />
			</label>
		);
	}

	if (adding) {
		return (
			<p className="mt-2.5 flex items-center justify-end gap-1.5 text-[11px] text-fg-subtle">
				<Spinner className="size-3" />
				Adding to Steam…
			</p>
		);
	}

	if (confirming) {
		return (
			<div className="mt-2.5 flex max-w-64 flex-col items-end gap-1.5 text-right">
				<p className="text-[11px] leading-snug text-fg-subtle">
					If Steam is open, Unalia closes it for a few seconds, adds the game, then reopens it.
				</p>
				<div className="flex gap-1.5">
					<Button size="sm" variant="secondary" onClick={() => setConfirming(false)}>
						Cancel
					</Button>
					<Button size="sm" variant="outline" onClick={() => void add()}>
						Add to Steam
					</Button>
				</div>
			</div>
		);
	}

	return (
		<button
			type="button"
			onClick={() => setConfirming(true)}
			title="Launch it from Steam to get the Steam overlay and FPS counter"
			className="mt-2.5 ml-auto flex cursor-pointer items-center gap-1 text-[11px] text-fg-subtle underline-offset-2 transition-colors hover:text-fg hover:underline"
		>
			Add to Steam for its overlay
		</button>
	);
}
