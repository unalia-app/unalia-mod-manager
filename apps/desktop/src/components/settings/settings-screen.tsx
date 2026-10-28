"use client";

import { cn } from "@modalia/ui";
import { ChevronLeft, HardDrive, LifeBuoy, Link2, Monitor } from "lucide-react";
import { useRouter } from "next/navigation";
import { useState } from "react";

import { AppHeader } from "@/components/app-header";
import { Connections } from "@/components/settings/connections";
import { Display } from "@/components/settings/display";
import { Storage } from "@/components/settings/storage";
import { Support } from "@/components/settings/support";

const sections = [
	{ id: "connections", label: "Connections", icon: <Link2 className="size-3.5" /> },
	{ id: "display", label: "Display", icon: <Monitor className="size-3.5" /> },
	{ id: "storage", label: "Storage", icon: <HardDrive className="size-3.5" /> },
	{ id: "support", label: "Support", icon: <LifeBuoy className="size-3.5" /> },
] as const;

type Section = (typeof sections)[number]["id"];

/** Settings (taken from Unalia's Settings page): linked accounts, display, storage, support. */
export function SettingsScreen() {
	const router = useRouter();
	const [section, setSection] = useState<Section>("connections");

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
						{sections.map((s) => (
							<button
								type="button"
								key={s.id}
								onClick={() => setSection(s.id)}
								className={cn(
									"flex w-full cursor-pointer items-center gap-2 rounded-md px-3 py-2.5 text-xs transition-colors",
									section === s.id ? "bg-surface-strong text-fg" : "text-fg-subtle hover:bg-white/5 hover:text-fg",
								)}
							>
								{s.icon}
								{s.label}
							</button>
						))}
					</nav>
				</aside>

				<section key={section} className="flex min-w-0 flex-1 animate-fade-up flex-col gap-6">
					{section === "connections" && <Connections />}
					{section === "display" && <Display />}
					{section === "storage" && <Storage />}
					{section === "support" && <Support />}
				</section>
			</div>
		</main>
	);
}
