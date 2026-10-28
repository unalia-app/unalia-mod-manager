"use client";

import { Check, Clock, X } from "lucide-react";

import { toast } from "@/components/toaster";
import { type PendingOp, cancelPending, splitError } from "@/lib/engine";
import { useGameStatus } from "@/lib/game-status";

/**
 * Changes requested during a session: applied automatically when the game
 * closes, cancelable one by one.
 */
export function PendingBanner({ appId }: { appId: number }) {
	const status = useGameStatus(appId);
	const pending = status?.pending ?? [];
	if (pending.length === 0) return null;

	const count = `${pending.length} change${pending.length > 1 ? "s" : ""}`;
	const cancel = (op: PendingOp) =>
		cancelPending(appId, op.modId).catch((error: unknown) => {
			const { message } = splitError(error);
			toast({ title: message, tone: "error" });
		});

	return (
		<div className="mt-6 flex animate-row-in flex-wrap items-center gap-x-4 gap-y-2 rounded-xl border border-accent/25 bg-accent/[0.07] px-4 py-3">
			<span className="flex items-center gap-2 text-sm font-medium text-fg">
				<Clock className="size-4 text-accent" />
				{status?.running
					? `${count} will apply when you close the game`
					: `${count} waiting to be applied`}
			</span>
			<ul className="flex flex-wrap gap-1.5">
				{pending.map((op) => (
					<li
						key={op.modId}
						className="flex items-center gap-1 rounded-full border border-line-strong bg-surface-strong py-0.5 pr-1 pl-2.5 text-xs text-fg-muted"
					>
						{describe(op)}
						{op.type === "install" && op.ready && (
							<Check className="size-3 text-success" aria-label="Downloaded" />
						)}
						<button
							type="button"
							aria-label={`Cancel: ${describe(op)}`}
							onClick={() => cancel(op)}
							className="cursor-pointer rounded-full p-0.5 text-fg-faint transition-colors hover:bg-white/10 hover:text-fg"
						>
							<X className="size-3" />
						</button>
					</li>
				))}
			</ul>
		</div>
	);
}

export function describe(op: PendingOp): string {
	switch (op.type) {
		case "install":
			return `Install ${op.name}`;
		case "setEnabled":
			return `${op.enabled ? "Turn on" : "Turn off"} ${op.name}`;
		case "uninstall":
			return `Remove ${op.name}`;
	}
}
