use std::process::Command;

fn main() {
    vergen().unwrap();
    linker_be_nice();
    // make sure linkall.x is the last linker script (otherwise might cause problems with flip-link)
    println!("cargo:rustc-link-arg=-Tlinkall.x");
    let profile = std::env::var("PROFILE").unwrap();
    println!("cargo:rerun-if-changed=target/xtensa-esp32-none-elf/{profile}/cancomponents");
    create_esp32_image();
}

fn create_esp32_image() {
    let profile = std::env::var("PROFILE").unwrap();
    let target_binary = format!("target/xtensa-esp32-none-elf/{profile}/cancomponents");
    let ota_binary = format!("target/xtensa-esp32-none-elf/{profile}/cancomponents.bin");

    if let Err(e) = Command::new("espflash")
        .args(["save-image", "--chip", "esp32", &target_binary, &ota_binary])
        .status()
    {
        println!("cargo:warning=Failed to create ESP32 image: {e}");
        // Kein panic! - Build soll auch ohne Image weiterlaufen
    }
}

fn vergen() -> Result<(), Box<dyn std::error::Error>> {
    let git2 = vergen_git2::Git2Builder::all_git()?;
    vergen_git2::Emitter::default()
        .add_instructions(&git2)?
        .emit()?;

    Ok(())
}

fn linker_be_nice() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() > 1 {
        let kind = &args[1];
        let what = &args[2];

        match kind.as_str() {
            "undefined-symbol" => match what.as_str() {
                "_defmt_timestamp" => {
                    eprintln!();
                    eprintln!("💡 `defmt` not found - make sure `defmt.x` is added as a linker script and you have included `use defmt_rtt as _;`");
                    eprintln!();
                }
                "_stack_start" => {
                    eprintln!();
                    eprintln!("💡 Is the linker script `linkall.x` missing?");
                    eprintln!();
                }
                _ => (),
            },
            // we don't have anything helpful for "missing-lib" yet
            _ => {
                std::process::exit(1);
            }
        }

        std::process::exit(0);
    }

    println!(
        "cargo:rustc-link-arg=-Wl,--error-handling-script={}",
        std::env::current_exe().unwrap().display()
    );
}
