"use client";

import { useNexusAccount } from "@/lib/nexus";

/**
 * Without a Nexus account, you can browse mods but not download them. No
 * account can be linked until Nexus approves Unalia's login.
 */
export function NexusConnect() {
	const account = useNexusAccount();
	if (account !== null) return null;

	return (
		<div className="mb-6 flex max-w-2xl animate-row-in flex-wrap items-center gap-x-4 gap-y-2 rounded-xl border border-dashed border-line px-4 py-3">
			<p className="min-w-0 flex-1 text-sm text-fg-muted">
				Downloads from Nexus Mods are paused until Nexus approves Unalia's login.{" "}
				<span className="text-fg-subtle">You can still browse mods, and frameworks like RED4ext install as usual.</span>
			</p>
		</div>
	);
}
