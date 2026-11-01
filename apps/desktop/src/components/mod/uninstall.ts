import { toast } from "@/components/toaster";
import { splitError } from "@/lib/engine";
import { remove, removeMany, restore, restoreMany } from "@/lib/installs";

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
				details: "Its files are gone from the game.",
				action: { label: "Undo", onClick: () => restore(appId, modId).catch(fail) },
			});
		}
	} catch (error) {
		fail(error);
	}
}

/** Removes several mods, with one "Undo" for all of them. */
export async function uninstallManyWithUndo(appId: number, modIds: string[]): Promise<void> {
	const { removed, failed } = await removeMany(appId, modIds);
	if (removed.length > 0) {
		toast({
			title: `${removed.length} mod${removed.length === 1 ? "" : "s"} removed`,
			details: "Their files are gone from the game.",
			action: { label: "Undo", onClick: () => restoreMany(appId, removed).catch(fail) },
		});
	}
	if (failed) fail(failed.reason);
}
