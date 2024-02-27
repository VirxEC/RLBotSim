use std::{env::set_current_dir, io, path::Path, process::Command};

const FLATC_BINARY: &str = if cfg!(windows) { "flatc.exe" } else { "flatc" };
const OUT_FOLDER: &str = "./src/generated";
const SCHEMA_FOLDER: &str = "./flatbuffers-schema";

fn main() -> io::Result<()> {
    println!("cargo:rerun-if-changed=flatbuffers-schema/comms.fbs");
    println!("cargo:rerun-if-changed=flatbuffers-schema/event.fbs");
    println!("cargo:rerun-if-changed=flatbuffers-schema/gamestate.fbs");
    println!("cargo:rerun-if-changed=flatbuffers-schema/matchstart.fbs");
    println!("cargo:rerun-if-changed=flatbuffers-schema/rendering.fbs");
    println!("cargo:rerun-if-changed=flatbuffers-schema/rlbot.fbs");

    set_current_dir(env!("CARGO_MANIFEST_DIR"))?;

    let schema_folder = Path::new(SCHEMA_FOLDER);
    assert!(schema_folder.exists(), "Could not find flatbuffers schema folder");

    let schema_folder_str = schema_folder.display();

    Command::new(format!("{schema_folder_str}/{FLATC_BINARY}"))
        .args([
            "--rust",
            "--gen-object-api",
            "--gen-all",
            "--filename-suffix",
            "",
            "--rust-module-root-file",
            "-o",
            OUT_FOLDER,
            &format!("{schema_folder_str}/rlbot.fbs"),
        ])
        .spawn()?
        .wait()?;

    let out_folder = Path::new(OUT_FOLDER).join("rlbot").join("flat");

    assert!(
        out_folder.exists(),
        "Could not find generated folder: {}",
        out_folder.display()
    );

    Ok(())
}
