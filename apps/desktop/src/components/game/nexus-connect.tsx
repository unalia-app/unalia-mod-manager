"use client";

import { Button } from "@modalia/ui";
import { KeyRound, LogOut } from "lucide-react";
import { type FormEvent, useState } from "react";

import { toast } from "@/components/toaster";
import { splitError } from "@/lib/engine";
import { connectNexus, disconnectNexus, useNexusAccount } from "@/lib/nexus";
import { openExternal } from "@/lib/open";

const API_KEYS_PAGE = "https://next.nexusmods.com/settings/api-keys";

/**
 * Compte Nexus Mods : la clé API personnelle de l'utilisateur, gardée sur
 * sa machine. Sans compte, on peut parcourir les mods mais pas les
 * télécharger.
 */
export function NexusConnect() {
	const account = useNexusAccount();
	const [key, setKey] = useState("");
	const [busy, setBusy] = useState(false);
	const [error, setError] = useState<string | null>(null);

	if (account === undefined) return null;

	if (account) {
		return (
			<p className="mb-4 flex items-center gap-2 text-xs text-fg-subtle">
				Nexus Mods: {account.name} · {account.premium ? "Premium, downloads start right here" : "free account, you confirm each download on Nexus"}
				<button
					type="button"
					onClick={() => void disconnectNexus()}
					className="ml-1 inline-flex cursor-pointer items-center gap-1 text-fg-faint transition-colors hover:text-fg"
				>
					<LogOut className="size-3" />
					Disconnect
				</button>
			</p>
		);
	}

	const submit = async (event: FormEvent) => {
		event.preventDefault();
		setBusy(true);
		setError(null);
		try {
			const connected = await connectNexus(key);
			setKey("");
			toast({ title: `Connected to Nexus Mods as ${connected.name}` });
		} catch (e) {
			setError(splitError(e).message);
		} finally {
			setBusy(false);
		}
	};

	return (
		<form onSubmit={submit} className="glass mb-6 max-w-2xl rounded-xl p-4">
			<p className="flex items-center gap-2 text-sm font-medium text-fg">
				<KeyRound className="size-4 text-accent" />
				Connect your Nexus Mods account
			</p>
			<p className="mt-1 text-xs leading-relaxed text-fg-subtle">
				Nexus needs an account to download mods. Copy your personal API key from{" "}
				<button
					type="button"
					onClick={() => void openExternal(API_KEYS_PAGE)}
					className="cursor-pointer text-fg-muted underline underline-offset-2 hover:text-fg"
				>
					your Nexus settings
				</button>{" "}
				(at the bottom of the page) and paste it here. It stays on this computer. Modalia then opens the "Mod Manager
				Download" links of the site.
			</p>
			<div className="mt-3 flex gap-2">
				<input
					type="password"
					value={key}
					onChange={(e) => setKey(e.target.value)}
					placeholder="Personal API key"
					autoComplete="off"
					spellCheck={false}
					className="h-9 min-w-0 flex-1 rounded-md border border-line-strong bg-surface-strong px-3 text-sm text-fg outline-none placeholder:text-fg-faint focus:border-white/30 focus:ring-2 focus:ring-white/20"
				/>
				<Button type="submit" size="sm" disabled={busy || !key.trim()}>
					{busy ? "Checking…" : "Connect"}
				</Button>
			</div>
			{error && <p className="mt-2 text-xs text-danger">{error}</p>}
		</form>
	);
}
