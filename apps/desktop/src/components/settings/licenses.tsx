"use client";

import { Button } from "@modalia/ui";
import { FileText } from "lucide-react";

import { toast } from "@/components/toaster";
import { openLicense, splitError } from "@/lib/engine";

/** The app's license and those of the projects it relies on. */
export function Licenses() {
	const open = (license: "app" | "thirdParty") =>
		openLicense(license).catch((error: unknown) => {
			const { message, details } = splitError(error);
			toast({ title: "Couldn't open the license", details: details ?? message, tone: "error" });
		});

	return (
		<div className="rounded-xl border border-line bg-white/[0.03] p-5 sm:p-6">
			<h3 className="text-sm font-semibold text-fg">Licenses</h3>
			<p className="mt-0.5 max-w-xl text-xs leading-relaxed text-fg-subtle">
				Unalia Mod Manager is free software under the GNU GPL v3. It is built on open source projects, each under its
				own license, and reads <code>.rar</code> archives with UnRAR by Alexander Roshal.
			</p>
			<div className="mt-4 flex flex-wrap items-center gap-2">
				<Button variant="outline" leftIcon={<FileText className="size-3.5" />} onClick={() => open("thirdParty")}>
					Open source licenses
				</Button>
				<Button variant="outline" leftIcon={<FileText className="size-3.5" />} onClick={() => open("app")}>
					GNU GPL v3
				</Button>
			</div>
		</div>
	);
}
