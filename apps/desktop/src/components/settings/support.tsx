"use client";

import { Button } from "@modalia/ui";
import { Check, ClipboardCopy, FolderOpen } from "lucide-react";
import { useEffect, useState } from "react";

import { AppVersion } from "@/components/settings/app-version";
import { toast } from "@/components/toaster";
import { openLogsFolder, splitError } from "@/lib/engine";
import { buildReport } from "@/lib/report";

/** Signaler un problème : un rapport prêt à coller, et les logs à portée. */
export function Support() {
	const [report, setReport] = useState<string | null>(null);
	const [copied, setCopied] = useState(false);

	useEffect(() => {
		let current = true;
		buildReport().then(
			(text) => current && setReport(text),
			() => current && setReport(""),
		);
		return () => {
			current = false;
		};
	}, []);

	useEffect(() => {
		if (!copied) return;
		const timer = setTimeout(() => setCopied(false), 2000);
		return () => clearTimeout(timer);
	}, [copied]);

	const copy = async () => {
		try {
			// Rapport frais : le journal a pu bouger depuis l'ouverture.
			const text = await buildReport();
			setReport(text);
			await navigator.clipboard.writeText(text);
			setCopied(true);
		} catch (error) {
			const { message, details } = splitError(error);
			toast({ title: "Couldn't copy the report", details: details ?? message, tone: "error" });
		}
	};

	const openLogs = () =>
		openLogsFolder().catch((error: unknown) => {
			const { message, details } = splitError(error);
			toast({ title: "Couldn't open the logs folder", details: details ?? message, tone: "error" });
		});

	return (
		<>
			<div className="space-y-1">
				<h1 className="text-2xl font-semibold tracking-tight">Support</h1>
				<p className="text-sm text-fg-subtle">Something went wrong? A report helps fix it fast.</p>
			</div>

			<AppVersion />

			<div className="rounded-xl border border-line bg-white/[0.03] p-5 sm:p-6">
				<h3 className="text-sm font-semibold text-fg">Report a problem</h3>
				<p className="mt-0.5 max-w-xl text-xs leading-relaxed text-fg-subtle">
					Copies your app and Windows versions, your games and their mods, and the recent logs. Paste it in your bug
					report. It never includes your account name, your API key or your Windows user name.
				</p>
				<div className="mt-4 flex flex-wrap items-center gap-2">
					<Button
						leftIcon={copied ? <Check className="size-3.5" /> : <ClipboardCopy className="size-3.5" />}
						onClick={copy}
						disabled={report === null}
					>
						{copied ? "Copied" : "Copy report"}
					</Button>
					<Button variant="outline" leftIcon={<FolderOpen className="size-3.5" />} onClick={openLogs}>
						Open logs folder
					</Button>
				</div>

				{report && (
					<details className="mt-5 text-xs text-fg-faint">
						<summary className="cursor-pointer select-none transition-colors hover:text-fg-subtle">
							See what's in the report
						</summary>
						<pre className="mt-2 max-h-80 overflow-auto whitespace-pre-wrap rounded-md border border-line bg-black/30 p-3 font-mono text-[11px] text-fg-subtle">
							{report}
						</pre>
					</details>
				)}
			</div>
		</>
	);
}
