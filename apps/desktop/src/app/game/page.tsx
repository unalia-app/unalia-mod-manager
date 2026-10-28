import { Suspense } from "react";

import { GameScreen } from "@/components/game/game-screen";

// The game id is passed as a parameter (`/game/?id=1030300`): with the
// static export, a single page serves every game.
export default function GamePage() {
	return (
		<Suspense>
			<GameScreen />
		</Suspense>
	);
}
