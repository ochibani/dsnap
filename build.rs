use clap::ValueEnum;
use clap_complete::{generate_to, Shell};
use clap_mangen::Man;
use std::io::Error;
use std::path::{Path, PathBuf};

include!("src/cli.rs");

fn render_man(cmd: clap::Command, out_dir: &Path, root: bool) -> Result<(), Error> {
    let name = cmd.get_name().to_string();
    let title = if root {
        "dsnap".to_string()
    } else {
        format!("dsnap-{}", name)
    };
    let filename = format!("{}.8", title);
    let aliases: Vec<String> = cmd
        .get_all_aliases()
        .map(|alias| alias.to_string())
        .collect();
    let subcommands: Vec<clap::Command> = cmd
        .get_subcommands()
        .filter(|sub| !sub.is_hide_set() && sub.get_name() != "help")
        .cloned()
        .collect();

    let man = Man::new(cmd)
        .title(title)
        .section("8")
        .source(format!("dsnap {}", env!("CARGO_PKG_VERSION")))
        .manual("System Administration Utilities");

    let mut buffer: Vec<u8> = Vec::new();
    man.render(&mut buffer)?;

    if !aliases.is_empty() {
        buffer.extend_from_slice(format!(".SH ALIASES\n{}.\n", aliases.join(", ")).as_bytes());
    }

    if root {
        buffer.extend_from_slice(include_str!("packaging/man/dsnap-extra.roff").as_bytes());
    }

    std::fs::write(out_dir.join(filename), &buffer)?;

    for subcommand in subcommands {
        render_man(subcommand, out_dir, false)?;
    }

    Ok(())
}

// Build shell completions and manual pages
fn main() -> Result<(), Error> {
    let out_dir = match std::env::var_os("OUT_DIR") {
        None => return Ok(()),
        Some(out_dir) => PathBuf::from(out_dir),
    };

    let mut cmd = cli();
    for &shell in Shell::value_variants() {
        generate_to(shell, &mut cmd, "dsnap", &out_dir)?;
    }

    for entry in std::fs::read_dir(&out_dir)? {
        let entry = entry?;
        let filename = entry.file_name();
        let filename = filename.to_string_lossy();
        if filename.starts_with("dsnap") && filename.ends_with(".8") {
            std::fs::remove_file(entry.path())?;
        }
    }

    render_man(cmd, &out_dir, true)?;

    println!("cargo:rerun-if-changed=src/cli.rs");
    println!("cargo:rerun-if-changed=packaging/man/dsnap-extra.roff");

    Ok(())
}
