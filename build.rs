//! # Build Script do Engine
//!
//! Este arquivo é executado pelo Cargo durante o processo de compilação,
//! antes de compilar o código Rust principal. Build scripts são usados
//! para gerar código ou realizar outras tarefas de build.
//!
//! # O que este build script faz?
//!
//! 1. Compila arquivos Protocol Buffers (.proto) em código Rust
//! 2. Tenta localizar o compilador protoc automaticamente

fn main() {
    // Diz ao Cargo para reexecutar este script se o arquivo .proto mudar
    println!("cargo:rerun-if-changed=proto/engine.proto");

    // Tenta localizar o protoc no workspace se não estiver no PATH
    if std::env::var("PROTOC").is_err() {
        let mut workspace_protoc = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        workspace_protoc.push("protoc-bin");
        workspace_protoc.push("bin");
        workspace_protoc.push("protoc.exe");

        if workspace_protoc.exists() {
            println!("cargo:warning=Usando protoc do workspace: {:?}", workspace_protoc);
            std::env::set_var("PROTOC", workspace_protoc);
        }
    }

    // Compila os arquivos .proto usando prost_build
    prost_build::compile_protos(
        &["proto/engine.proto"],
        &["proto/"],
    ).expect("Falha ao compilar Protobuf. Certifique-se que o 'protoc' está instalado ou disponível no workspace (protoc-bin/).");
}
