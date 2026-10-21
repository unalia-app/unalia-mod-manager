"use client";

import { Button } from "@modalia/ui";
import { Link2 } from "lucide-react";
import { useRouter } from "next/navigation";

import { useNexusAccount } from "@/lib/nexus";

/** Sans compte Nexus, on parcourt les mods mais on ne peut pas les télécharger. */
export function NexusConnect() {
	const account = useNexusAccount();
	const router = useRouter();
	if (account !== null) return null;

	return (
		<div className="mb-6 flex max-w-2xl animate-row-in flex-wrap items-center gap-x-4 gap-y-2 rounded-xl border border-dashed border-line px-4 py-3">
			<p className="min-w-0 flex-1 text-sm text-fg-muted">
				Link your Nexus Mods account to download mods.{" "}
				<span className="text-fg-subtle">Frameworks like RED4ext install without it.</span>
			</p>
			<Button size="sm" leftIcon={<Link2 className="size-3.5" />} onClick={() => router.push("/settings/")}>
				Link Nexus Mods
			</Button>
		</div>
	);
}
