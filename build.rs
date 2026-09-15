fn main() {
    println!("cargo:rerun-if-changed=proto/engine.proto");
    println!("cargo:rerun-if-env-changed=PROTOC");

    if std::env::var_os("PROTOC").is_none() && !protoc_no_path() {
        match protoc_bin_vendored::protoc_bin_path() {
            Ok(path) => {
                println!("cargo:warning=usando protoc vendorizado: {}", path.display());
                std::env::set_var("PROTOC", path);
            }
            Err(e) => {
                println!("cargo:warning=protoc vendorizado indisponível ({e}); tentando PATH");
            }
        }
    }

    prost_build::compile_protos(&["proto/engine.proto"], &["proto/"])
        .expect("falha ao compilar proto/engine.proto");
}

fn protoc_no_path() -> bool {
    std::process::Command::new("protoc")
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}
