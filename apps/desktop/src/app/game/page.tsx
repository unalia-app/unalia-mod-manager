import { Suspense } from "react";

import { GameScreen } from "@/components/game/game-screen";

// L'identifiant du jeu passe en paramètre (`/game/?id=1030300`) : avec
// l'export statique, une seule page sert tous les jeux.
export default function GamePage() {
	return (
		<Suspense>
			<GameScreen />
		</Suspense>
	);
}
