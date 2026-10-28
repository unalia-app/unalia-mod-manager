import { toast } from "@/components/toaster";
import { splitError } from "@/lib/engine";
import { remove, restore } from "@/lib/installs";

function fail(error: unknown) {
	const { message, details } = splitError(error);
	toast({ title: message, details: details ?? undefined, tone: "error" });
}

/**
 * Retire un mod, partout de la même façon (Mes mods, page du mod, carte) :
 * un toast offre « Undo » (SPEC §3.5). Jeu lancé : le retrait attend sa
 * fermeture, et le bandeau d'attente le montre déjà, avec de quoi annuler.
 */
export async function uninstallWithUndo(appId: number, modId: string, name: string): Promise<void> {
	try {
		const outcome = await remove(appId, modId);
		if (outcome === "done") {
			toast({
				title: `${name} removed`,
				action: { label: "Undo", onClick: () => restore(appId, modId).catch(fail) },
			});
		}
	} catch (error) {
		fail(error);
	}
}
