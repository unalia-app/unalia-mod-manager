import { toast } from "@/components/toaster";
import { splitError } from "@/lib/engine";
import { remove, restore } from "@/lib/installs";

function fail(error: unknown) {
	const { message, details } = splitError(error);
	toast({ title: message, details: details ?? undefined, tone: "error" });
}

/**
 * Removes a mod, the same way everywhere (My mods, mod page, card): a
 * toast offers "Undo". Game running: the removal waits for it to close,
 * and the pending banner already shows it, with a way to cancel.
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
