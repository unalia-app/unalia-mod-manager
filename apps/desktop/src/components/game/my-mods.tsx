import { Button } from "@modalia/ui";
import { Layers } from "lucide-react";

import { Notice } from "@/components/notice";

/** Onglet Mes mods. L'installation arrive avec les briques resolver et deploy. */
export function MyMods({ onDiscover }: { onDiscover: () => void }) {
	return (
		<div className="glass max-w-xl">
			<Notice
				icon={<Layers className="size-5 text-fg-muted" />}
				title="No mods installed yet"
				action={
					<Button size="sm" onClick={onDiscover}>
						Discover mods
					</Button>
				}
			>
				Mods you install show up here. Turn them on, off or remove them in one click.
			</Notice>
		</div>
	);
}
