import { isTauri } from "@tauri-apps/api/core";
import { openUrl } from "@tauri-apps/plugin-opener";

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
