import { Inter_Tight, Poppins } from "next/font/google";

// Mêmes polices qu'Unalia. next/font les télécharge au build et les sert
// depuis l'appli : aucun appel réseau à l'exécution.
export const poppins = Poppins({
	subsets: ["latin"],
	weight: ["300", "400", "500", "600", "700"],
	variable: "--font-poppins",
});

export const interTight = Inter_Tight({
	subsets: ["latin"],
	variable: "--font-inter-tight",
});
