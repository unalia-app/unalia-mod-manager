import { useEffect, useState } from "react";

import { gameAccent } from "@/lib/engine";

/** CSS accent of the game (`null`: neutral), computed by the engine. */
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
