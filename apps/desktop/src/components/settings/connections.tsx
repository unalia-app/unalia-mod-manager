"use client";

import { Button, cn } from "@modalia/ui";
import { Check, LogOut } from "lucide-react";
import { type ReactNode, useState } from "react";

import { toast } from "@/components/toaster";
import { splitError } from "@/lib/engine";
import { disconnectNexus, loginNexus, useNexusStatus } from "@/lib/nexus";

/** Accounts of the mod sources, modeled on Unalia's Connections. */
export function Connections() {
	return (
		<>
			<div className="space-y-1">
				<h1 className="text-2xl font-semibold tracking-tight">Connections</h1>
				<p className="text-sm text-fg-subtle">Link your mod site accounts to download and update mods from Unalia.</p>
			</div>

			<div className="space-y-4">
				<NexusCard />

				<SourceRow
					name="Thunderstore"
					letter="T"
					color="#23ffb0"
					detail="No account needed. Mods install in one click."
				/>
				<SourceRow
					name="GitHub"
					letter="G"
					color="#f0f6fc"
					detail="No account needed. Used for frameworks published by their authors, like RED4ext."
				/>

				<div className="pt-2">
					<p className="mb-3 text-xs font-medium tracking-wider text-fg-faint uppercase">Coming soon</p>
					<div className="grid gap-3 sm:grid-cols-2">
						<ComingSoon name="CurseForge" color="#f16436" />
						<ComingSoon name="Modrinth" color="#1bd96a" />
						<ComingSoon name="mod.io" color="#07c1d8" />
						<ComingSoon name="GameBanana" color="#ffd200" />
					</div>
				</div>
			</div>
		</>
	);
}

function NexusCard() {
	const status = useNexusStatus();
	const account = status?.account ?? null;
	const linked = account !== null || status?.offline === true;
	const [busy, setBusy] = useState(false);
	const [confirmDisconnect, setConfirmDisconnect] = useState(false);

	const fail = (error: unknown) => {
		const { message, details } = splitError(error);
		toast({ title: message, details: details ?? undefined, tone: "error" });
	};

	const connect = async () => {
		setBusy(true);
		try {
			const connected = await loginNexus();
			toast({ title: `Connected to Nexus Mods as ${connected.name}`, details: "Downloads from Nexus Mods now use your account." });
		} catch (error) {
			fail(error);
		} finally {
			setBusy(false);
		}
	};

	const disconnect = async () => {
		setBusy(true);
		try {
			await disconnectNexus();
			toast({ title: "Nexus Mods account disconnected", details: "Connect it again to download from Nexus Mods." });
		} catch (error) {
			fail(error);
		} finally {
			setBusy(false);
			setConfirmDisconnect(false);
		}
	};

	return (
		<ConnectionCard linked={linked} accent="#d98f40">
			<div className="flex items-center justify-between gap-4">
				<div className="flex items-center gap-3.5">
					<Tile letter="N" color="#d98f40" strong={linked} />
					<div>
						<div className="flex items-center gap-2">
							<h3 className="text-sm font-semibold text-fg">Nexus Mods</h3>
							{linked && <ConnectedPill />}
						</div>
						<p className="mt-0.5 text-xs text-fg-subtle">
							{status?.offline
								? `${account ? `${account.name} · ` : ""}Nexus Mods can't be reached right now`
								: account
									? `${account.name} · ${account.premium ? "Premium: downloads start right in Unalia" : "Free account: you confirm each download on Nexus"}`
									: "The largest mod library, with Cyberpunk 2077"}
						</p>
					</div>
				</div>
				{account?.avatarUrl && (
					<img src={account.avatarUrl} alt="" className="size-9 rounded-full ring-1 ring-line-strong" />
				)}
			</div>

			<div className="mt-5">
				{status === undefined ? null : linked ? (
					confirmDisconnect ? (
						<div className="flex flex-wrap items-center gap-2">
							<span className="text-xs text-fg-muted">Unalia won't be able to download from Nexus anymore.</span>
							<Button size="sm" variant="outline" disabled={busy} onClick={() => void disconnect()}>
								Disconnect
							</Button>
							<Button size="sm" variant="secondary" onClick={() => setConfirmDisconnect(false)}>
								Cancel
							</Button>
						</div>
					) : (
						<Button
							size="sm"
							variant="secondary"
							leftIcon={<LogOut className="size-3.5" />}
							className="text-fg-subtle hover:text-danger"
							onClick={() => setConfirmDisconnect(true)}
						>
							Disconnect
						</Button>
					)
				) : status?.sso ? (
					<Button size="sm" disabled={busy} onClick={() => void connect()}>
						{busy ? "Waiting for Nexus…" : "Connect Nexus Mods"}
					</Button>
				) : (
					// Nexus doesn't allow personal API keys in apps: the login
					// comes with its approval, nothing to link until then.
					<p className="max-w-xl text-xs leading-relaxed text-fg-subtle">
						Nexus Mods asks apps to use its own login, and Unalia is waiting for its approval. Until then, you can
						browse Nexus mods and see their updates, but not download them from Unalia.
					</p>
				)}
				{busy && status?.sso && (
					<p className="mt-2 text-xs text-fg-subtle">Click Authorise on the Nexus Mods page that opened in your browser.</p>
				)}
			</div>
		</ConnectionCard>
	);
}

function ConnectionCard({ linked, accent, children }: { linked: boolean; accent: string; children: ReactNode }) {
	return (
		<div
			className={cn(
				"relative w-full overflow-hidden rounded-xl border transition-colors duration-300",
				linked ? "border-line bg-white/[0.03]" : "border-dashed border-line hover:border-line-strong hover:bg-white/[0.02]",
			)}
		>
			{/* Colored edge line at the top, like in Unalia. */}
			{linked && (
				<div
					className="absolute inset-x-0 top-0 h-px"
					style={{ background: `linear-gradient(to right, transparent, ${accent}80, transparent)` }}
				/>
			)}
			<div className="p-5 sm:p-6">{children}</div>
		</div>
	);
}

function SourceRow({ name, letter, color, detail }: { name: string; letter: string; color: string; detail: string }) {
	return (
		<div className="flex items-center gap-3.5 rounded-xl border border-line bg-white/[0.015] px-5 py-4">
			<Tile letter={letter} color={color} />
			<div className="min-w-0 flex-1">
				<p className="text-sm font-medium text-fg">{name}</p>
				<p className="mt-0.5 text-xs text-fg-subtle">{detail}</p>
			</div>
			<span className="inline-flex items-center gap-1 rounded-full bg-success/10 px-2 py-0.5 text-[10px] font-medium text-success">
				<Check className="size-2.5" strokeWidth={3} />
				Ready
			</span>
		</div>
	);
}

function ComingSoon({ name, color }: { name: string; color: string }) {
	return (
		<div className="flex select-none items-center gap-3.5 rounded-xl border border-line/50 bg-white/[0.015] px-4 py-3.5 opacity-40">
			<Tile letter={name[0] ?? "?"} color={color} small />
			<p className="flex-1 text-xs font-medium text-fg-muted">{name}</p>
			<span className="rounded-full bg-white/5 px-2 py-0.5 text-[10px] text-fg-faint">Soon</span>
		</div>
	);
}

function Tile({ letter, color, strong = false, small = false }: { letter: string; color: string; strong?: boolean; small?: boolean }) {
	return (
		<span
			className={cn("flex shrink-0 items-center justify-center rounded-lg font-bold", small ? "size-8 text-xs" : "size-10 text-sm")}
			style={{ backgroundColor: `${color}${strong ? "33" : "1f"}`, color }}
		>
			{letter}
		</span>
	);
}

function ConnectedPill() {
	return (
		<span className="inline-flex items-center gap-1 rounded-full bg-success/10 px-2 py-0.5 text-[10px] font-medium text-success">
			<Check className="size-2.5" strokeWidth={3} />
			Connected
		</span>
	);
}
