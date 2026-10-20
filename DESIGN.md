# Modalia : règles de design

Référence visuelle pour les humains et les agents IA. Le langage vient d'**Unalia** (unalia.app) : on le prolonge, on ne le réinvente pas. En cas de doute, ouvrir Unalia et faire pareil.

Tokens : `packages/ui/src/styles.css`. Composants : `packages/ui/src/`.

## Principes

1. **Sombre, une seule surface.** Fond `bg` (#0d0d0f) partout. La profondeur vient de transparences blanches et de flou, jamais de gris opaques empilés.
2. **Le blanc hiérarchise.** Titre en `fg`, contenu en `fg-muted`, métadonnées en `fg-subtle`, détails techniques en `fg-faint`. Pas d'autre couleur de texte, sauf les états.
3. **Seul l'accent change selon le jeu** (SPEC §4). Il est défini par `--accent` sur un conteneur et s'utilise via `accent`. Structure, typo, espacements et composants ne bougent jamais d'un jeu à l'autre.
4. **Chaque état est dessiné** : chargement (squelettes), vide, erreur, indisponible. Pas de spinner seul au milieu d'un écran vide, pas de code d'erreur en premier plan.
5. **Calme.** Une action principale par écran (bouton blanc `primary`). Le reste en `secondary` ou `outline`.

## Tokens

| Rôle | Classe | Valeur |
|---|---|---|
| Fond | `bg-bg` | `#0d0d0f` |
| Surface (panneau) | `bg-surface` | blanc 3 % |
| Surface survolée / active | `bg-surface-hover` | blanc 6 % |
| Surface marquée (champ, chip) | `bg-surface-strong` | blanc 10 % |
| Bordure | `border-line` | blanc 8 % |
| Bordure marquée | `border-line-strong` | blanc 20 % |
| Texte | `text-fg` / `-muted` / `-subtle` / `-faint` | blanc 100 / 60 / 40 / 25 % |
| Accent du jeu | `accent` | `--accent`, neutre par défaut |
| États | `success` `info` `warning` `danger` | emerald / blue / amber / red 400 |

Une couleur d'état s'emploie toujours en **contour 20 % + fond 10 % + texte plein** (voir `Badge`), jamais en aplat.

L'accent sera calculé en OKLCH depuis la jaquette du jeu, avec un contraste garanti. Ne jamais coder un accent en dur dans un composant.

## Typographie

- **Poppins** (`font-sans`, par défaut) pour tout le texte.
- **Inter Tight** (`font-tight`) pour les petits éléments denses : badges, raccourcis clavier, chiffres.
- Titres : `font-semibold tracking-tight`. Le titre principal d'un écran suit le motif d'Unalia : première ligne en `fg`, suite en `fg-muted`.
- Tailles courantes : titre d'écran `text-5xl`, texte `text-sm`, métadonnées `text-xs`, badges `text-[11px]`.

## Formes

- Rayons : `rounded-md` pour boutons et champs, `rounded-lg` pour lignes et vignettes, `rounded-xl` pour panneaux, `rounded-2xl` pour fenêtres modales.
- Panneau standard : utilitaire `glass` (bordure `line`, fond `surface`, flou). Le réserver aux panneaux de taille raisonnable : `backdrop-filter` coûte cher sur de grandes surfaces (SPEC §4).

## Mouvement

- Animer uniquement `transform` et `opacity`.
- Courbe `ease-out` (`cubic-bezier(0.32, 0.72, 0, 1)`, celle des modales d'Unalia) pour les entrées et les panneaux. Les durées vont de 150 ms (survol) à 500 ms (entrée d'écran).
- Entrées : `animate-fade-up` pour les blocs d'écran (décalés de 60 ms), `animate-row-in` pour les lignes de liste (décalées de 30 ms).
- Chargement : `Skeleton` (reflet `animate-shimmer`), aux dimensions du contenu final.
- Bouton principal : le libellé glisse au survol (effet repris du `MainButton` d'Unalia).
- `prefers-reduced-motion` est respecté globalement.

## Composants (`@modalia/ui`)

| Composant | Usage |
|---|---|
| `Button` | `primary` (blanc, une seule action principale), `secondary` (texte), `outline` (verre). Tailles `sm` `md` `lg` `icon`. `isLoading` remplace le contenu par un `Spinner`. |
| `Badge` | Étiquette en pilule : `neutral`, `accent` ou un état. |
| `SearchField` | Champ de recherche en verre clair, avec un élément optionnel à droite. |
| `Skeleton` | Bloc de chargement. |
| `Kbd` | Touche ou raccourci clavier. |
| `cn` | Fusion de classes, connaît les tokens maison. |

Avant de créer un composant, vérifier qu'il n'existe pas dans Unalia (`components/ui/`) : on le porte ici plutôt que d'en dessiner un nouveau.

## Fenêtre

- Pas de barre de titre système sous macOS (`titleBarStyle: Overlay`). Le haut de la fenêtre (`h-11`) sert de zone de déplacement, et le contenu de l'en-tête commence après les feux tricolores (`pl-22`).
- Sous Windows : effets Mica/acrylique à évaluer.

## Textes

- Tutoiement, phrases courtes, sans jargon : « Ajoute un jeu en choisissant son dossier », pas « Sélectionnez un répertoire d'installation ».
- Une erreur dit ce qui s'est passé et quoi faire. Le détail technique va dans un `<details>` « Détails techniques ».
- Pas de tiret cadratin ni demi-cadratin.
