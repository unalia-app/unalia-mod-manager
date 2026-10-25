# Archives RAR

Utilisées par `crates/mod-engine/tests/install.rs`. Aucun outil libre ne sait créer de `.rar` : ces archives viennent des données de test de la crate [`unrar`](https://github.com/muja/unrar.rs) (MIT ou Apache-2.0).

| Fichier | Contenu |
|---|---|
| `version.rar` | Un seul fichier, `VERSION`, qui contient `unrar-0.4.0`. |
| `crypted.rar` | Archive protégée par mot de passe. |
| `archive.part1.rar` | Premier volume d'une archive en plusieurs parties, sans les suivants. |
