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

### Accent du jeu

- Calculé par le moteur (`mod-engine::artwork`) à partir de la jaquette du cache de Steam. On prend la teinte dominante, avec une luminosité OKLCH fixe de 0,78 et un chroma entre 0,07 et 0,17. La couleur est ramenée dans le gamut sRGB, avec un contraste d'au moins 4,5:1 sur `bg`. Une jaquette grise ne produit pas d'accent, et l'interface reste alors neutre.
- Un conteneur pose `style={{ "--accent": … }}` et l'utilitaire `accent-transition` : `--accent` est déclaré en `@property <color>`, ce qui donne un fondu de 700 ms quand on change de jeu.
- Ne jamais coder un accent en dur dans un composant : `text-accent`, `bg-accent/10`, `border-accent/20`.

### Ambiance

- La bannière du jeu (`hero`), floutée (`blur-2xl`) à 35 % d'opacité, s'estompe vers le bas, avec un halo radial teinté par l'accent par-dessus.
- L'ambiance suit le jeu que l'utilisateur désigne (survol, flèches), jamais un jeu choisi par défaut.
- Images : uniquement via `artworkUrl()` (protocole `artwork` du moteur), jamais par un chemin de fichier.

## Typographie

- **Poppins** (`font-sans`, par défaut) pour tout le texte.
- **Inter Tight** (`font-tight`) pour les petits éléments denses : badges, raccourcis clavier, chiffres.
- Titres : `font-semibold tracking-tight`. Le titre principal d'un écran suit le motif d'Unalia : première ligne en `fg`, suite en `fg-muted`.
- Tailles courantes : titre d'écran `text-5xl`, texte `text-sm`, métadonnées `text-xs`, badges `text-[11px]`.

## Formes

- Rayons : `rounded-md` pour boutons et champs, `rounded-lg` pour lignes et vignettes, `rounded-xl` pour panneaux, `rounded-2xl` pour fenêtres modales.
- Panneau standard : utilitaire `glass` (bordure `line`, fond `surface`, flou). Le réserver aux panneaux de taille raisonnable : `backdrop-filter` coûte cher sur de grandes surfaces (SPEC §4).
- **Contrôle posé sur une image** (bouton ou badge sur une icône de mod ou une jaquette) : utilitaire `liquid-glass`, un verre sombre teinté avec un liseré clair, lisible sur une image claire comme sombre. Jamais de bouton blanc sur une image : il disparaît sur un fond clair.
- Barres de défilement : la barre native est masquée (sous Windows, elle réserve sa colonne même transparente). La page a sa barre en surimpression (`OverlayScrollbar`), visible pendant le défilement ou au survol du bord droit, et qu'on peut attraper.
- **Jamais de `glass` sur un élément répété ou animé** (cartes d'une grille, ligne qui se soulève) : le flou est recalculé à chaque image, et le survol saccade. Utiliser `rounded-xl border border-line bg-surface`, qui donne quasiment le même rendu sur fond sombre.

## Mouvement

- Animer uniquement `transform` et `opacity`.
- Courbe `ease-out` (`cubic-bezier(0.32, 0.72, 0, 1)`, celle des modales d'Unalia) pour les entrées et les panneaux. Les durées vont de 150 ms (survol) à 500 ms (entrée d'écran).
- Entrées : `animate-fade-up` pour les blocs d'écran (décalés de 60 ms), `animate-row-in` pour les lignes de liste (décalées de 30 ms).
- Chargement : `Skeleton` (reflet `animate-shimmer`), aux dimensions du contenu final.
- Bouton principal : le libellé glisse au survol (effet repris du `MainButton` d'Unalia).
- `prefers-reduced-motion` est respecté globalement.
- **Intro de lancement** (`components/intro.tsx`, environ 1,6 s) : les lettres de MODALIA apparaissent une à une (fondu, légère montée, du flou au net, jamais masquées : une lettre à moitié coupée se lit comme un bug), un reflet irisé les traverse, un halo s'ouvre et une ligne se trace. Le logo s'envole ensuite vers l'en-tête (FLIP : on part d'un grand texte et on le réduit, pour garder des lettres nettes). Pendant l'intro, les entrées `animate-fade-up` de l'écran sont en pause (`.intro-pending`) et reprennent au moment de l'envol. Un clic ou une touche passe l'intro. Elle se joue une fois par lancement.
- Piège Tailwind v4 : `translate-*`, `scale-*` et `rotate-*` utilisent les propriétés CSS `translate`, `scale` et `rotate`, pas `transform`.
  - Une transition explicite doit les nommer : `transition-[opacity,translate]`, pas `transition-[opacity,transform]`, sinon l'effet saute au lieu de s'animer. `transition-transform` et `transition` les couvrent déjà.
  - Sur un élément animé par une keyframe en `transform`, ne pas mettre de classe `translate-*` : les deux s'additionnent.

## Composants (`@modalia/ui`)

| Composant | Usage |
|---|---|
| `Button` | `primary` (blanc, une seule action principale), `secondary` (texte), `outline` (verre). Tailles `sm` `md` `lg` `icon`. `isLoading` remplace le contenu par un `Spinner`. |
| `Badge` | Étiquette en pilule : `neutral`, `accent` ou un état. |
| `SearchField` | Champ de recherche en verre clair, avec un élément optionnel à droite. |
| `Segmented` | Choix exclusif à indicateur glissant : `tabs` (onglets d'écran, soulignés à l'accent) ou `pills` (tri, filtres ; hauteur h-10 comme un `SearchField` compact, pour s'aligner dans une barre d'outils). |
| `Skeleton` | Bloc de chargement. |
| `Kbd` | Touche ou raccourci clavier. |
| `cn` | Fusion de classes, connaît les tokens maison. |

Avant de créer un composant, vérifier qu'il n'existe pas dans Unalia (`components/ui/`) : on le porte ici plutôt que d'en dessiner un nouveau.

## Toasts

- **Rares.** Seulement pour ce qui n'a pas d'autre place à l'écran : « Undo » après un retrait, ou une erreur sans endroit où s'afficher. Jamais pour confirmer une réussite que l'écran montre déjà.
- Pastille sombre opaque, en bas au centre, sans flou ni liseré coloré. Action en texte à l'accent (« Undo »). Erreur : petite icône rouge, détail technique replié derrière « Details ».
- Pas de croix : le toast part seul après 4 s, le survol met le minuteur en pause. Deux au plus à l'écran.
- Les détails d'une erreur liée à un élément (bouton Install…) se déplient sur place, pas dans un toast.

## Fenêtre

- Pas de barre de titre système sous macOS (`titleBarStyle: Overlay`). L'en-tête (`AppHeader`) est la zone de déplacement (`data-tauri-drag-region` sur l'élément lui-même, ses boutons restent cliquables). Il reste collé en haut, avec un fond flouté une fois la page défilée. Son contenu commence après les feux tricolores (`pl-22`).
- Un élément `fixed` (barre flottante, menu) se rend dans `document.body` via `createPortal` : un ancêtre animé en `transform` (les entrées `animate-fade-up`) devient sinon sa référence, et l'élément sort de l'écran.
- Cartes de mods : petit bouton « Install » au survol (visible en continu pendant l'installation ou après un échec), sans mode sélection. Le moteur installe dépendances et loader tout seul.
- `overflow-clip` plutôt que `overflow-hidden` sur les conteneurs d'écran : `hidden` crée un conteneur de défilement et casse `position: sticky`.
- Un texte tronqué (`truncate`) coupe aussi les jambages (g, j, p) : lui donner un `pb-1`.
- Sous Windows : effets Mica/acrylique à évaluer.

## Textes

- **L'interface est en anglais.** Le code, ses commentaires et cette documentation restent en français. Une vraie traduction communautaire viendra plus tard (SPEC §10).
- Ton direct, phrases courtes, sans jargon : « Add a game by picking its folder », pas « Select an installation directory ».
- Une erreur dit ce qui s'est passé et quoi faire (« Couldn't load mods. Check your connection. »). Le détail technique va dans un `<details>` « Technical details ».
- Casse de phrase, y compris pour les titres et les boutons : « Add a game manually », pas « Add A Game Manually ».
- Guillemets typographiques “ ” autour d'une saisie de l'utilisateur.
- Pas de tiret cadratin ni demi-cadratin.
