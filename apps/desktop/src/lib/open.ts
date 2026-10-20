import { isTauri } from "@tauri-apps/api/core";
import { openUrl } from "@tauri-apps/plugin-opener";

/**
 * Ouvre une page web dans le navigateur du système, jamais dans la fenêtre
 * de Modalia (qui a accès aux commandes du moteur). HTTPS uniquement, comme
 * la capability `opener:allow-open-url`.
 */
export async function openExternal(url: string): Promise<void> {
	if (!url.startsWith("https://")) return;
	if (isTauri()) await openUrl(url);
	else window.open(url, "_blank", "noopener,noreferrer");
}
