//! `plugins.txt` des jeux Bethesda, tenu à jour d'après les plugins que
//! Modalia a posés (`loaders::bethesda::sync_plugins`).

use std::fs;
use std::path::Path;

use mod_engine::loaders::bethesda::sync_plugins;

/// Plugin minimal : en-tête `TES4` de Skyrim SE avec ses masters.
fn plugin(path: &Path, masters: &[&str]) {
    let mut body = Vec::new();
    for master in masters {
        let mut name = master.as_bytes().to_vec();
        name.push(0);
        body.extend_from_slice(b"MAST");
        body.extend_from_slice(&(name.len() as u16).to_le_bytes());
        body.extend_from_slice(&name);
        body.extend_from_slice(b"DATA\x08\x00");
        body.extend_from_slice(&[0; 8]);
    }
    let mut file = b"TES4".to_vec();
    file.extend_from_slice(&(body.len() as u32).to_le_bytes());
    file.extend_from_slice(&[0; 16]);
    file.extend_from_slice(&body);
    fs::write(path, file).unwrap();
}

fn deployed(names: &[&str]) -> Vec<String> {
    names.iter().map(|n| format!("Data/{n}")).collect()
}

#[test]
fn plugins_txt_follows_the_mods() {
    let game = tempfile::tempdir().unwrap();
    let local = tempfile::tempdir().unwrap();
    let data = game.path().join("Data");
    fs::create_dir(&data).unwrap();
    plugin(&data.join("Skyrim.esm"), &[]);
    plugin(&data.join("Manual.esp"), &[]);
    plugin(&data.join("Patch.esp"), &["Skyrim.esm", "Lib.esm"]);
    plugin(&data.join("Lib.esm"), &["Skyrim.esm"]);
    let list = local
        .path()
        .join("Skyrim Special Edition")
        .join("plugins.txt");

    // Le jeu n'a jamais été lancé : le fichier est créé.
    let ours = deployed(&["Patch.esp", "Lib.esm"]);
    assert!(sync_plugins(game.path(), &list, &ours).unwrap());
    assert_eq!(
        fs::read_to_string(&list).unwrap(),
        "*Lib.esm\r\n*Patch.esp\r\n"
    );

    // Lignes du joueur gardées (même inactives), commentaires compris ;
    // celles dont le fichier a disparu partent.
    fs::write(
        &list,
        "# This file is used by Skyrim to keep track of your downloaded content.\r\nManual.esp\r\n*Gone.esp\r\n*Patch.esp\r\n",
    )
    .unwrap();
    assert!(sync_plugins(game.path(), &list, &ours).unwrap());
    assert_eq!(
        fs::read_to_string(&list).unwrap(),
        "# This file is used by Skyrim to keep track of your downloaded content.\r\nManual.esp\r\n*Lib.esm\r\n*Patch.esp\r\n"
    );
    // Rien à changer : le fichier n'est pas réécrit.
    assert!(!sync_plugins(game.path(), &list, &ours).unwrap());

    // Mod désactivé : son plugin quitte `Data/`, puis la liste.
    fs::remove_file(data.join("Patch.esp")).unwrap();
    assert!(sync_plugins(game.path(), &list, &deployed(&["Lib.esm"])).unwrap());
    assert_eq!(
        fs::read_to_string(&list).unwrap(),
        "# This file is used by Skyrim to keep track of your downloaded content.\r\nManual.esp\r\n*Lib.esm\r\n"
    );
}

#[test]
fn a_missing_game_leaves_plugins_txt_alone() {
    let game = tempfile::tempdir().unwrap();
    let local = tempfile::tempdir().unwrap();
    let list = local.path().join("plugins.txt");
    fs::write(&list, "*Mod.esp\r\n").unwrap();
    assert!(!sync_plugins(&game.path().join("gone"), &list, &deployed(&["Mod.esp"])).unwrap());
    assert_eq!(fs::read_to_string(&list).unwrap(), "*Mod.esp\r\n");
}
