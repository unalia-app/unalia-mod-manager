"use client";

import { isTauri } from "@tauri-apps/api/core";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { useEffect } from "react";

import { applyInterfaceSize } from "@/lib/display";

/**
 * Applique la taille d'interface choisie dès l'ouverture (voir `lib/display`),
 * puis à chaque changement d'écran : la taille automatique dépend de l'écran.
 */
export function DisplayScale() {
	useEffect(() => {
		void applyInterfaceSize();
		if (!isTauri()) return;
		// Déplacer la fenêtre envoie un événement par pixel : on attend la fin.
		let timer: ReturnType<typeof setTimeout> | undefined;
		const settle = () => {
			clearTimeout(timer);
			timer = setTimeout(() => void applyInterfaceSize(), 300);
		};
		const current = getCurrentWindow();
		const unlisten = Promise.all([current.onMoved(settle), current.onScaleChanged(settle)]);
		return () => {
			clearTimeout(timer);
			void unlisten.then((fns) => {
				for (const fn of fns) fn();
			});
		};
	}, []);
	return null;
}
