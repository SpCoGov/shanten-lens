use std::{env, path::PathBuf};

fn main() {
    let protoc = protoc_bin_vendored::protoc_bin_path().expect("vendored protoc unavailable");
    env::set_var("PROTOC", protoc);
    let out = PathBuf::from(env::var("OUT_DIR").expect("OUT_DIR missing"));
    prost_build::Config::new()
        .file_descriptor_set_path(out.join("liqi_desc.bin"))
        .compile_protos(&["../proto/liqi.proto"], &["../proto"])
        .expect("failed to compile liqi.proto");
    println!("cargo:rerun-if-changed=../proto/liqi.proto");
}
