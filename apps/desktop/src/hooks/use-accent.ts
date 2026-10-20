import { useEffect, useState } from "react";

import { gameAccent } from "@/lib/engine";

/** Accent CSS du jeu (`null` : neutre), calculé par le moteur. */
export function useAccent(appId: number | null): string | null {
	const [accent, setAccent] = useState<string | null>(null);

	useEffect(() => {
		if (appId === null) {
			setAccent(null);
			return;
		}
		let current = true;
		gameAccent(appId).then((value) => current && setAccent(value));
		return () => {
			current = false;
		};
	}, [appId]);

	return accent;
}
