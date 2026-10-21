// Jamais de console sous Windows, même en dev : un lien `nxm://` relance
// l'appli (qui passe le lien à l'exemplaire déjà ouvert puis s'arrête), et
// une console ferait un flash à l'écran. Les logs de dev arrivent quand même
// dans le terminal de `tauri dev`, qui lit la sortie du processus.
#![windows_subsystem = "windows"]

fn main() {
    modalia_desktop_lib::run()
}
