import { Suspense } from "react";

import { ModScreen } from "@/components/mod/mod-screen";

// `/mod/?game=1030300&id=namespace-Nom` : une page statique pour tous les mods.
export default function ModPage() {
	return (
		<Suspense>
			<ModScreen />
		</Suspense>
	);
}
