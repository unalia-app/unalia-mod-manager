"use client";

import { useEffect } from "react";

import { dismiss, toast } from "@/components/toaster";
import { checkForUpdate, installUpdate } from "@/lib/app-update";
import { splitError } from "@/lib/engine";

/** Laisse l'accueil se charger avant d'interroger le serveur des releases. */
const CHECK_AFTER_MS = 5_000;

/**
 * Au démarrage, une nouvelle version de l'appli s'annonce par un toast. Pas
 * d'erreur si la recherche échoue (hors ligne) : Settings > Support permet
 * de réessayer.
 */
export function AppUpdateCheck() {
	useEffect(() => {
		if (process.env.NODE_ENV === "development") return;
		const timer = window.setTimeout(() => {
			checkForUpdate().then(
				(update) => {
					if (!update) return;
					toast({
						title: `Version ${update.version} is available`,
						action: { label: "Update", onClick: () => void updateNow() },
						duration: 15_000,
					});
				},
				() => {},
			);
		}, CHECK_AFTER_MS);
		return () => window.clearTimeout(timer);
	}, []);

	return null;
}

/** Installe depuis un toast : l'appli se ferme et se relance si tout va bien. */
async function updateNow() {
	const downloading = toast({ title: "Downloading the update…", duration: 60_000 });
	try {
		await installUpdate();
	} catch (error) {
		dismiss(downloading);
		const { message, details } = splitError(error);
		toast({ title: message, details: details ?? undefined, tone: "error", duration: 8_000 });
	}
}
