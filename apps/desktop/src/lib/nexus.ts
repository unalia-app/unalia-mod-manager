import { useEffect, useSyncExternalStore } from "react";

import { type NexusAccount, type NexusStatus, nexusConnect, nexusDisconnect, nexusLogin, nexusStatus } from "@/lib/engine";

/** Nexus state of the session: `undefined` while it is unknown. */
let status: NexusStatus | undefined;
let loading: Promise<void> | null = null;
const listeners = new Set<() => void>();

function set(next: NexusStatus) {
	status = next;
	for (const listener of listeners) listener();
}

function subscribe(listener: () => void) {
	listeners.add(listener);
	return () => listeners.delete(listener);
}

function load() {
	if (loading) return;
	// Key refused: behave as logged out.
	loading = nexusStatus()
		.then(set, () => set({ account: null, sso: false, offline: false }))
		.finally(() => {
			loading = null;
		});
}

// Nexus unreachable at startup: check the account again when the network is back.
if (typeof window !== "undefined") {
	window.addEventListener("online", () => {
		if (status?.offline) load();
	});
}

export function useNexusStatus(): NexusStatus | undefined {
	const value = useSyncExternalStore(subscribe, () => status, () => undefined);
	useEffect(() => {
		if (status === undefined) load();
	}, []);
	return value;
}

/**
 * Logged-in account: `null` without an account, `undefined` while we don't
 * know. Offline, the last recognized account; never checked, `undefined`.
 */
export function useNexusAccount(): NexusAccount | null | undefined {
	const value = useNexusStatus();
	if (value === undefined) return undefined;
	return value.account ?? (value.offline ? undefined : null);
}

function connected(account: NexusAccount) {
	set({ sso: status?.sso ?? false, offline: false, account });
	return account;
}

export async function loginNexus(): Promise<NexusAccount> {
	return connected(await nexusLogin());
}

export async function connectNexus(apiKey: string): Promise<NexusAccount> {
	return connected(await nexusConnect(apiKey));
}

export async function disconnectNexus() {
	await nexusDisconnect();
	set({ sso: status?.sso ?? false, offline: false, account: null });
}
