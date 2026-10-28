import { Suspense } from "react";

import { ModScreen } from "@/components/mod/mod-screen";

// `/mod/?game=1030300&id=namespace-Name`: one static page for every mod.
export default function ModPage() {
	return (
		<Suspense>
			<ModScreen />
		</Suspense>
	);
}
