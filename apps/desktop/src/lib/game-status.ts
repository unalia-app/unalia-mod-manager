import { useEffect, useSyncExternalStore } from "react";

import { toast } from "@/components/toaster";
import {
	type GameStatus,
	type PendingOp,
	gameStatus,
	onGameStatus,
	onPendingApplied,
	playGame,
	splitError,
} from "@/lib/engine";
import { refreshInstalled, settleBackgroundInstalls } from "@/lib/installs";

/** Game state (running, starting, pending changes), pushed by the engine. */

let statuses = new Map<number, GameStatus>();
const listeners = new Set<() => void>();

function set(status: GameStatus) {
	statuses = new Map(statuses).set(status.appId, status);
	for (const l of listeners) l();
}

let listening = false;
function listenOnce() {
	if (listening) return;
	listening = true;
	void onGameStatus(set);
	// Success: the screen updates, no need for a toast. Only failures are
	// reported, for lack of another place to show them.
	void onPendingApplied(({ appId, failed }) => {
		void refreshInstalled(appId).finally(() => settleBackgroundInstalls(appId));
		for (const f of failed) {
			const { message, details } = splitError(f.message);
			toast({ title: message, details: details ?? undefined, tone: "error", duration: 8000 });
		}
	});
}

function subscribe(listener: () => void) {
	listeners.add(listener);
	listenOnce();
	return () => listeners.delete(listener);
}

export function useGameStatus(appId: number): GameStatus | undefined {
	const status = useSyncExternalStore(subscribe, () => statuses.get(appId), () => undefined);
	useEffect(() => {
		void gameStatus(appId).then(set, () => {});
	}, [appId]);
	return status;
}

/** Pending change for this mod, if there is one. */
export function usePendingOp(appId: number, modId: string): PendingOp | undefined {
	return useGameStatus(appId)?.pending.find((op) => op.modId === modId);
}

export async function play(appId: number) {
	try {
		await playGame(appId);
	} catch (error) {
		const { message, details } = splitError(error);
		toast({ title: message, details: details ?? undefined, tone: "error" });
	}
}
