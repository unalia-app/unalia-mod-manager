import { useEffect, useSyncExternalStore } from "react";

import { type NexusAccount, nexusAccount, nexusConnect, nexusDisconnect } from "@/lib/engine";

/** Compte Nexus de la session : `undefined` tant qu'il n'est pas connu. */
let account: NexusAccount | null | undefined;
let loading: Promise<void> | null = null;
const listeners = new Set<() => void>();

function set(next: NexusAccount | null) {
	account = next;
	for (const listener of listeners) listener();
}

function subscribe(listener: () => void) {
	listeners.add(listener);
	return () => listeners.delete(listener);
}

export function useNexusAccount(): NexusAccount | null | undefined {
	const value = useSyncExternalStore(subscribe, () => account, () => undefined);
	useEffect(() => {
		if (account !== undefined || loading) return;
		// Clé refusée ou Nexus injoignable : on se comporte comme déconnecté.
		loading = nexusAccount()
			.then(set, () => set(null))
			.finally(() => {
				loading = null;
			});
	}, []);
	return value;
}

export async function connectNexus(apiKey: string): Promise<NexusAccount> {
	const connected = await nexusConnect(apiKey);
	set(connected);
	return connected;
}

export async function disconnectNexus() {
	await nexusDisconnect();
	set(null);
}
