"use client";

import { useEffect } from "react";

import { applyInterfaceSize } from "@/lib/display";

/** Applique la taille d'interface choisie dès l'ouverture (voir `lib/display`). */
export function DisplayScale() {
	useEffect(() => {
		void applyInterfaceSize();
	}, []);
	return null;
}
