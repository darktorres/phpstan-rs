// Walks frame pointers only when the whole program keeps them: on x86_64,
// with `-C force-frame-pointers=yes` among the build's flags.
fn main() {
    println!("cargo::rustc-check-cfg=cfg(dhat_frame_pointers)");
    println!("cargo::rerun-if-env-changed=CARGO_ENCODED_RUSTFLAGS");
    let flags = std::env::var("CARGO_ENCODED_RUSTFLAGS").unwrap_or_default();
    let flags: Vec<&str> = flags.split('\x1f').collect();
    let forced = flags.iter().enumerate().any(|(i, flag)| {
        let value = match flag.strip_prefix("-C") {
            Some("") => flags.get(i + 1).copied().unwrap_or(""),
            Some(rest) => rest,
            None => flag.strip_prefix("--codegen=").unwrap_or(""),
        };
        matches!(
            value,
            "force-frame-pointers=yes"
                | "force-frame-pointers=y"
                | "force-frame-pointers=on"
                | "force-frame-pointers=true"
        )
    });
    let x86_64 = std::env::var("CARGO_CFG_TARGET_ARCH").is_ok_and(|arch| arch == "x86_64");
    if forced && x86_64 {
        println!("cargo::rustc-cfg=dhat_frame_pointers");
    }
}
