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

export function useNexusStatus(): NexusStatus | undefined {
	const value = useSyncExternalStore(subscribe, () => status, () => undefined);
	useEffect(() => {
		if (status !== undefined || loading) return;
		// Clé refusée ou Nexus injoignable : on se comporte comme déconnecté.
		loading = nexusStatus()
			.then(set, () => set({ account: null, sso: false }))
			.finally(() => {
				loading = null;
			});
	}, []);
	return value;
}

/** Compte connecté : `null` sans compte, `undefined` tant qu'on ne sait pas. */
export function useNexusAccount(): NexusAccount | null | undefined {
	const value = useNexusStatus();
	return value === undefined ? undefined : value.account;
}

function connected(account: NexusAccount) {
	set({ sso: status?.sso ?? false, account });
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
	set({ sso: status?.sso ?? false, account: null });
}
