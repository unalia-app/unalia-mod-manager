import { Button } from "@modalia/ui";
import { Layers } from "lucide-react";

import { Notice } from "@/components/notice";

/** Onglet Mes mods. L'installation arrive avec les briques resolver et deploy. */
export function MyMods({ onDiscover }: { onDiscover: () => void }) {
	return (
		<div className="glass max-w-xl">
			<Notice
				icon={<Layers className="size-5 text-fg-muted" />}
				title="Aucun mod installé pour l'instant"
				action={
					<Button size="sm" onClick={onDiscover}>
						Découvrir des mods
					</Button>
				}
			>
				Les mods que tu installes apparaîtront ici. Tu pourras les activer, les désactiver ou les retirer en un clic.
			</Notice>
		</div>
	);
}
