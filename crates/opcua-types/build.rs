use std::env;
use std::path::Path;

use opcua_codegen::{CodeGenConfig, CodeGenSource, CodeGenTarget, TypeCodeGenTarget, run_codegen};

fn main() {
    let out_dir = env::var("OUT_DIR").unwrap();
    let target_dir = format!("{out_dir}/opcua_generated");

    println!("cargo:rerun-if-changed=schemas/");
    println!("cargo:rustc-env=OPCUA_GENERATED_DIR={target_dir}");

    let sources = vec![
        CodeGenSource::Implicit("./schemas".into()),
        CodeGenSource::Implicit("./schemas/UA-1.05.03-2023-12-15".into()),
    ];
    let targets = vec![CodeGenTarget::Types(TypeCodeGenTarget {
        file: "SiOME Nodeset.xml".to_string(),
        output_dir: target_dir.into(),
        node_ids_from_nodeset: true,
        ..Default::default()
    })];
    let codegen_config = CodeGenConfig {
        extra_header: String::new(),
        preferred_locale: "en-US".to_string(),
        targets,
        sources,
    };

    let manifest_dir = env::var_os("CARGO_MANIFEST_DIR").unwrap();

    run_codegen(&codegen_config, Path::new(&manifest_dir)).unwrap();
}
