import { isTauri } from "@tauri-apps/api/core";
import { openUrl } from "@tauri-apps/plugin-opener";

import { toast } from "@/components/toaster";

/**
 * Opens a web page in the system browser, never in the Modalia window
 * (which has access to the engine commands). HTTPS only, like the
 * `opener:allow-open-url` capability.
 */
export async function openExternal(url: string): Promise<void> {
	if (!url.startsWith("https://")) return;
	if (isTauri()) await openUrl(url);
	else window.open(url, "_blank", "noopener,noreferrer");
}

/**
 * For a page that must be reached and won't open (no default browser in
 * Windows): pasted into a browser, it works the same.
 */
export async function copyLink(url: string): Promise<void> {
	try {
		await navigator.clipboard.writeText(url);
		toast({ title: "Link copied", details: "Paste it in your browser." });
	} catch {
		toast({ title: "Couldn't copy the link", tone: "error" });
	}
}
