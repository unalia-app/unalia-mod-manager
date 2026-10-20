# Faux dossiers Steam

Utilisés par `crates/mod-engine/tests/detect.rs`. Tous les fichiers de jeu sont vides : seuls leurs noms comptent.

Steam écrit des chemins absolus dans `libraryfolders.vdf`. Ici ils commencent par `{{FIXTURE}}`, que les tests remplacent par le chemin de leur copie temporaire de ce dossier (échappé comme le fait Steam).

| Dossier | Contenu |
|---|---|
| `root/` | Racine Steam. Hollow Knight: Silksong (Unity Mono), Steamworks Common Redistributables (moteur inconnu), un manifeste de jeu désinstallé, un manifeste corrompu, un `installdir` piégé, un dossier sans manifeste. |
| `library-d/` | Bibliothèque secondaire. Faux jeu Unreal, faux jeu Unity IL2CPP, doublon de Silksong. |
| `legacy/` | Racine Steam avec l'ancien format de `libraryfolders.vdf`. |

`disque-debranche/` est déclaré dans `root/steamapps/libraryfolders.vdf` mais n'existe volontairement pas.
