import { useEffect, useSyncExternalStore } from "react";

import { type NexusAccount, type NexusStatus, nexusConnect, nexusDisconnect, nexusLogin, nexusStatus } from "@/lib/engine";

/** État Nexus de la session : `undefined` tant qu'il n'est pas connu. */
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
	// Clé refusée : on se comporte comme déconnecté.
	loading = nexusStatus()
		.then(set, () => set({ account: null, sso: false, offline: false }))
		.finally(() => {
			loading = null;
		});
}

// Nexus injoignable au démarrage : on revérifie le compte au retour du réseau.
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
 * Compte connecté : `null` sans compte, `undefined` tant qu'on ne sait pas.
 * Hors ligne, le dernier compte reconnu ; jamais vérifié, `undefined`.
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
