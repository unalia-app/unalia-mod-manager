"use client";

import { cn } from "@modalia/ui";
import { ChevronLeft, Link2 } from "lucide-react";
import { useRouter } from "next/navigation";

import { AppHeader } from "@/components/app-header";
import { Connections } from "@/components/settings/connections";

const sections = [{ id: "connections", label: "Connections", icon: <Link2 className="size-3.5" /> }];

/** Réglages (repris de la page Settings d'Unalia) : comptes liés pour l'instant. */
export function SettingsScreen() {
	const router = useRouter();

	return (
		<main className="relative min-h-dvh overflow-x-clip">
			<AppHeader />
			<div className="relative z-10 mx-auto flex w-full max-w-5xl gap-10 px-8 pt-8 pb-20">
				<aside className="hidden w-48 shrink-0 md:block">
					<button
						type="button"
						onClick={() => router.back()}
						className="flex cursor-pointer items-center gap-2 rounded-md px-3 py-2 text-xs font-medium text-fg-subtle transition-colors hover:bg-white/10 hover:text-fg"
					>
						<ChevronLeft className="size-3.5" />
						Back
					</button>
					<nav className="mt-6 space-y-1">
						{sections.map((s, i) => (
							<span
								key={s.id}
								className={cn(
									"flex items-center gap-2 rounded-md px-3 py-2.5 text-xs",
									i === 0 ? "bg-surface-strong text-fg" : "text-fg-subtle",
								)}
							>
								{s.icon}
								{s.label}
							</span>
						))}
					</nav>
				</aside>

				<section className="flex min-w-0 flex-1 animate-fade-up flex-col gap-6">
					<Connections />
				</section>
			</div>
		</main>
	);
}
