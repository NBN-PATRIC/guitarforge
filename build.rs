fn main() {
    println!("cargo:rerun-if-changed=assets/guitarforge.rc");
    println!("cargo:rerun-if-changed=assets/guitarforge.ico");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        // ícone e nome do programa no .exe (Explorer, atalho, Gerenciador de Tarefas)
        embed_resource::compile("assets/guitarforge.rc", embed_resource::NONE)
            .manifest_optional()
            .expect("compilar assets/guitarforge.rc (precisa do rc.exe do Windows SDK)");
    }
}
