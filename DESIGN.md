# Modalia: design rules

Visual reference for humans and AI agents. The language comes from **Unalia** (unalia.app): we extend it, we don't reinvent it. When in doubt, open Unalia and do the same.

Tokens: `packages/ui/src/styles.css`. Components: `packages/ui/src/`.

## Name

- **Unalia** is the parent brand. This app is called **Unalia Mod Manager**: window title, installer, stores.
- Logo: "UNALIA" in bold italic, then "Mod manager" small below (uppercase, `fg-subtle`). Same layout in the intro, under the word.
- In UI sentences: "Unalia" alone ("Unalia installs it right after").
- "Modalia" stays the internal codename: repo, packages, `modalia.db` database. The app id is `app.unalia.modmanager` (the old `app.modalia.desktop` folder is taken over at startup).

## Principles

1. **Dark, a single surface.** `bg` background (#0d0d0f) everywhere. Depth comes from white transparencies and blur, never from stacked opaque grays.
2. **White sets the hierarchy.** Titles in `fg`, content in `fg-muted`, metadata in `fg-subtle`, technical details in `fg-faint`. No other text color, except states.
3. **Only the accent changes with the game.** It is set by `--accent` on a container and used through `accent`. Layout, type, spacing and components never change from one game to another.
4. **Every state is designed**: loading (skeletons), empty, error, unavailable. No lone spinner in the middle of an empty screen, no error code up front.
5. **Calm.** One primary action per screen (white `primary` button). Everything else in `secondary` or `outline`.

## Tokens

| Role | Class | Value |
|---|---|---|
| Background | `bg-bg` | `#0d0d0f` |
| Surface (panel) | `bg-surface` | white 3% |
| Hovered / active surface | `bg-surface-hover` | white 6% |
| Strong surface (field, chip) | `bg-surface-strong` | white 10% |
| Border | `border-line` | white 8% |
| Strong border | `border-line-strong` | white 20% |
| Text | `text-fg` / `-muted` / `-subtle` / `-faint` | white 100 / 60 / 40 / 25% |
| Game accent | `accent` | `--accent`, neutral by default |
| States | `success` `info` `warning` `danger` | emerald / blue / amber / red 400 |

A state color is always used as **20% outline + 10% background + solid text** (see `Badge`), never as a solid fill.

### Game accent

- Computed by the engine (`mod-engine::artwork`) from the cover in Steam's cache. It takes the dominant hue, with a fixed OKLCH lightness of 0.78 and a chroma between 0.07 and 0.17. The color is brought back into the sRGB gamut, with a contrast of at least 4.5:1 on `bg`. A gray cover gives no accent, and the UI then stays neutral.
- A container sets `style={{ "--accent": … }}` and the `accent-transition` utility: `--accent` is declared as an `@property <color>`, which gives a 700 ms fade when the game changes.
- Never hard-code an accent in a component: `text-accent`, `bg-accent/10`, `border-accent/20`.

### Ambience

- The game banner (`hero`), blurred (`blur-2xl`) at 35% opacity, fades out toward the bottom, with a radial glow tinted by the accent on top.
- The ambience follows the game the user points at (hover, arrows), never a game picked by default.
- Images: only through `artworkUrl()` (the engine's `artwork` protocol), never through a file path.

## Typography

- **Poppins** (`font-sans`, the default) for all text.
- **Inter Tight** (`font-tight`) for small dense elements: badges, keyboard shortcuts, numbers.
- Titles: `font-semibold tracking-tight`. The main title of a screen follows Unalia's pattern: first line in `fg`, the rest in `fg-muted`.
- Common sizes: screen title `text-5xl`, text `text-sm`, metadata `text-xs`, badges `text-[11px]`.

## Shapes

- Radii: `rounded-md` for buttons and fields, `rounded-lg` for rows and thumbnails, `rounded-xl` for panels, `rounded-2xl` for modal dialogs.
- Standard panel: the `glass` utility (`line` border, `surface` background, blur). Keep it for reasonably sized panels: `backdrop-filter` is expensive on large surfaces.
- **Control placed over an image** (button or badge on a mod icon or a cover): the `liquid-glass` utility, a tinted dark glass with a light edge, readable over a light or a dark image. Never a white button over an image: it disappears on a light background.
- Scrollbars: the native bar is hidden (on Windows, it reserves its column even when transparent). The page has its own overlay bar (`OverlayScrollbar`), shown while scrolling or when hovering the right edge, and it can be grabbed.
- **Never `glass` on a repeated or animated element** (cards of a grid, a row that lifts): the blur is recomputed at each frame, and hover stutters. Use `rounded-xl border border-line bg-surface`, which looks almost the same on a dark background.

## Motion

- Only animate `transform` and `opacity`.
- `ease-out` curve (`cubic-bezier(0.32, 0.72, 0, 1)`, the one of Unalia's dialogs) for entrances and panels. Durations go from 150 ms (hover) to 500 ms (screen entrance).
- Entrances: `animate-fade-up` for screen blocks (staggered by 60 ms), `animate-row-in` for list rows (staggered by 30 ms).
- Loading: `Skeleton` (`animate-shimmer` sheen), at the size of the final content.
- Primary button: the label slides on hover (effect taken from Unalia's `MainButton`).
- `prefers-reduced-motion` is respected globally.
- **Launch intro** (`components/intro.tsx`, about 1.6 s): the letters of UNALIA appear one by one (fade, slight rise, from blur to sharp, never clipped: a half-cut letter reads as a bug), an iridescent sheen crosses them, a glow opens and a line draws itself. The logo then flies to the header (FLIP: start from large text and scale it down, to keep the letters sharp). During the intro, the screen's `animate-fade-up` entrances are paused (`.intro-pending`) and resume when the flight starts. A click or a key skips the intro. It plays once per launch.
- Tailwind v4 trap: `translate-*`, `scale-*` and `rotate-*` use the CSS properties `translate`, `scale` and `rotate`, not `transform`.
  - An explicit transition must name them: `transition-[opacity,translate]`, not `transition-[opacity,transform]`, or the effect jumps instead of animating. `transition-transform` and `transition` already cover them.
  - On an element animated by a `transform` keyframe, don't add a `translate-*` class: the two add up.

## Components (`@modalia/ui`)

| Component | Use |
|---|---|
| `Button` | `primary` (white, a single primary action), `secondary` (text), `outline` (glass), `danger` (action that removes something, in a confirmation). Sizes `sm` `md` `lg` `icon`. `isLoading` replaces the content with a `Spinner`. |
| `Dialog` | Confirmation dialog, taken from Unalia's `AlertDialog`: `DialogHeader` (`DialogTitle`, `DialogDescription`), content, `DialogFooter` (Cancel as `secondary`, the action on the right). Escape and a click outside close it, except during the action (no `onClose`). |
| `Badge` | Pill label: `neutral`, `accent` or a state. |
| `SearchField` | Light glass search field, with an optional element on the right. |
| `Segmented` | Exclusive choice with a sliding indicator: `tabs` (screen tabs, underlined in the accent) or `pills` (sort, filters; h-10 height like a compact `SearchField`, to line up in a toolbar). |
| `Select` | Dropdown menu with a panel that detaches from the trigger. |
| `Skeleton` | Loading block. |
| `Kbd` | Key or keyboard shortcut. |
| `ToastStack` | The app's toasts (see below). |
| `GitHubIcon`, `DiscordIcon` | Brand icons (Simple Icons), in the text color. |
| `cn` | Class merging that knows the custom tokens. |

Before creating a component, check that it doesn't exist in Unalia (`components/ui/`): port it here rather than designing a new one.

## Toasts

- **Rare.** Only for what has no other place on screen: "Undo" after a removal, or an error with nowhere to show. Never to confirm a success the screen already shows.
- Opaque dark pill, bottom center, without blur or colored edge. Action as accent text ("Undo"). Error: small red icon, technical detail folded behind "Details".
- No cross: the toast leaves by itself after 4 s, hover pauses the timer. Two at most on screen.
- The details of an error tied to an element (Install button…) unfold in place, not in a toast.

## Window

- No system title bar. The header (`AppHeader`) is the drag area (`data-tauri-drag-region` on the element itself, its buttons stay clickable). It sticks to the top, with a blurred background once the page is scrolled. On macOS its content starts after the traffic lights (`pl-22`); on Windows, Modalia draws its own window buttons.
- A `fixed` element (floating bar, menu) renders into `document.body` through `createPortal`: otherwise an ancestor animated with `transform` (the `animate-fade-up` entrances) becomes its reference, and the element leaves the screen.
- Mod cards: small "Install" button on hover (always visible during the install or after a failure), no selection mode. The engine installs dependencies and the loader by itself.
- `overflow-clip` rather than `overflow-hidden` on screen containers: `hidden` creates a scroll container and breaks `position: sticky`.
- Truncated text (`truncate`) also cuts descenders (g, j, p): give it a `pb-1`.
- The credits pill (GitHub, Discord, author) sits bottom left on every page, small: the app is open source, not an ad for its maintainer.

## Copy

- **Everything is in English**: the interface, the code, its comments and the docs. A real community translation of the interface will come later.
- Direct tone, short sentences, no jargon: "Add a game by picking its folder", not "Select an installation directory".
- An error says what happened and what to do ("Couldn't load mods. Check your connection."). The technical detail goes in a "Technical details" `<details>`.
- Sentence case, including titles and buttons: "Add a game manually", not "Add A Game Manually".
- Curly quotes “ ” around user input.
- No em dash or en dash.
