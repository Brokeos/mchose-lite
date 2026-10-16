// Icône de l'exécutable (assets/icon.ico, reprise de MCHOSE HUB).
fn main() {
    println!("cargo:rerun-if-changed=assets/app.rc");
    println!("cargo:rerun-if-changed=assets/icon.ico");
    embed_resource::compile("assets/app.rc", embed_resource::NONE).manifest_optional().unwrap();
}
