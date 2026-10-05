//! The translation units of an image's build, and clang's dump of each.

use std::io::{BufReader, Read};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use serde::Deserialize;

/// A compilation database entry, its paths inside the image.
#[derive(Deserialize)]
struct Entry {
    directory: String,
    arguments: Vec<String>,
    file: String,
}

/// What a dump is made for: the image's own platform, or Windows.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Platform {
    Linux,
    Windows,
}

/// An exported image: the files the build read under `root`, with its
/// compile commands and compiler.
pub(crate) struct Export {
    pub(crate) root: PathBuf,
    /// The compiler the build used: its version and target triple.
    gcc_version: String,
    target: String,
    resource_dir: String,
    /// clang's file-system overlay, which puts the export's directories in
    /// place of the host's.
    overlay: PathBuf,
}

/// A translation unit as the catalog compiles it.
pub(crate) struct Unit {
    /// The source file, as the catalog names it.
    pub(crate) file: String,
    pub(crate) is_cpp: bool,
    /// Where clang runs, in the export.
    directory: PathBuf,
    /// clang's arguments, the paths in the export.
    arguments: Vec<String>,
    /// The build's options the catalog does not pass on.
    pub(crate) dropped: Vec<String>,
}

impl Export {
    pub(crate) fn open(dir: &Path) -> Result<Export, String> {
        let root = dir.join("root");
        let compiler = std::fs::read_to_string(dir.join("compiler.txt"))
            .map_err(|e| format!("{}: {e}", dir.join("compiler.txt").display()))?;
        let mut lines = compiler.lines();
        let gcc_version = lines.next().unwrap_or_default().trim().to_owned();
        let target = lines.next().unwrap_or_default().trim().to_owned();
        let output = Command::new("clang")
            .arg("-print-resource-dir")
            .output()
            .map_err(|e| format!("clang: {e}"))?;
        let resource_dir = String::from_utf8_lossy(&output.stdout).trim().to_owned();
        let overlay = dir.join("clang-overlay.yaml");
        write_overlay(&root, &resource_dir, &overlay)?;
        Ok(Export {
            root,
            gcc_version,
            target,
            resource_dir,
            overlay,
        })
    }

    /// The root as the dump's paths start.
    pub(crate) fn root_prefix(&self) -> String {
        self.root.to_string_lossy().into_owned()
    }

    fn in_root(&self, path: &str, directory: &str) -> String {
        let absolute = if path.starts_with('/') {
            path.to_owned()
        } else {
            format!("{directory}/{path}")
        };
        format!("{}{absolute}", self.root.display())
    }

    /// The build's translation units, in the database's order.
    pub(crate) fn units(&self, database: &Path) -> Result<Vec<Unit>, String> {
        let text = std::fs::read_to_string(database)
            .map_err(|e| format!("{}: {e}", database.display()))?;
        let entries: Vec<Entry> =
            serde_json::from_str(&text).map_err(|e| format!("{}: {e}", database.display()))?;
        Ok(entries.iter().map(|entry| self.unit(entry)).collect())
    }

    fn unit(&self, entry: &Entry) -> Unit {
        let file_in_image = if entry.file.starts_with('/') {
            entry.file.clone()
        } else {
            format!("{}/{}", entry.directory, entry.file)
        };
        let is_cpp = !file_in_image.ends_with(".c");
        let mut arguments = Vec::new();
        let mut dropped = Vec::new();
        let mut args = entry.arguments.iter().skip(1).peekable();
        while let Some(arg) = args.next() {
            let arg = arg.as_str();
            if arg == entry.file || arg == file_in_image {
                continue;
            }
            // Options with a path, written apart or joined.
            if let Some(option) = [
                "-I",
                "-isystem",
                "-iquote",
                "-idirafter",
                "-include",
                "-imacros",
            ]
            .into_iter()
            .find(|option| arg.starts_with(option))
            {
                let path = if arg == option {
                    match args.next() {
                        Some(path) => path.clone(),
                        None => continue,
                    }
                } else {
                    arg[option.len()..].to_owned()
                };
                arguments.push(option.to_owned());
                arguments.push(self.in_root(&path, &entry.directory));
                continue;
            }
            if arg == "-D" || arg == "-U" || arg == "-x" {
                if let Some(value) = args.next() {
                    arguments.push(arg.to_owned());
                    arguments.push(value.clone());
                }
                continue;
            }
            if arg == "-o" || arg == "-MF" || arg == "-MT" || arg == "-MQ" {
                args.next();
                continue;
            }
            let kept = arg.starts_with("-D")
                || arg.starts_with("-U")
                || arg.starts_with("-std=")
                || arg.starts_with("-O")
                || arg.starts_with("-m")
                || arg == "-pthread"
                || arg.starts_with("-fstack-protector")
                || arg.starts_with("-fvisibility=")
                || arg.starts_with("-ffp-contract=")
                || arg == "-fwrapv"
                || matches!(
                    arg,
                    "-fPIC"
                        | "-fpic"
                        | "-fPIE"
                        | "-fpie"
                        | "-fsigned-char"
                        | "-funsigned-char"
                        | "-fno-common"
                        | "-fno-builtin"
                        | "-ffreestanding"
                        | "-fopenmp"
                        | "-fexceptions"
                        | "-fno-exceptions"
                        | "-fno-rtti"
                );
            let ignored = arg == "-c"
                || arg == "-g"
                || arg.starts_with("-W")
                || arg.starts_with("-M")
                || arg.starts_with("-g")
                || arg == "-pipe"
                || arg == "-w";
            if kept {
                arguments.push(arg.to_owned());
            } else if !ignored {
                dropped.push(arg.to_owned());
            }
        }
        let file =
            crate::lower::catalog_path(&self.root_prefix(), &self.in_root(&file_in_image, "/"))
                .to_string();
        arguments.push(self.in_root(&file_in_image, &entry.directory));
        Unit {
            file,
            is_cpp,
            directory: PathBuf::from(self.in_root(&entry.directory, "/")),
            arguments,
            dropped,
        }
    }

    /// clang's whole command for a unit, the system headers searched in
    /// the build compiler's order, its version posed as.
    pub(crate) fn command(&self, unit: &Unit, platform: Platform) -> Command {
        let mut command = Command::new("clang");
        command.current_dir(&unit.directory);
        match platform {
            Platform::Linux => {
                command.arg(format!("--target={}", self.target));
                command.arg(format!("-fgnuc-version={}", self.gcc_version));
                // glibc takes these types from gcc from version 7 on, and
                // clang, posing as it, has none of them in C.
                command.args([
                    "-D_Float32=float",
                    "-D_Float64=double",
                    "-D_Float32x=double",
                    "-D_Float64x=long double",
                ]);
            }
            Platform::Windows => {
                command.arg("--target=x86_64-pc-windows-msvc");
            }
        }
        command.arg("-ivfsoverlay");
        command.arg(&self.overlay);
        command.args(["-nostdinc", "-isystem"]);
        command.arg(format!("{}/include", self.resource_dir));
        if platform == Platform::Linux {
            for dir in [
                "/usr/local/include".to_owned(),
                format!("/usr/include/{}", self.target),
                "/usr/include".to_owned(),
            ] {
                command.arg("-isystem");
                command.arg(self.in_root(&dir, "/"));
            }
        }
        // `offsetof`, and `__builtin_offsetof` where it is written alone,
        // keep the member they name beside them, an access the dump shows
        // and that adds nothing to the value: its address, which a flexible
        // array member has too. The macro's own name in it is the builtin.
        command.arg("-Doffsetof(t, d)=__builtin_offsetof(t, d)");
        command.arg(
            "-D__builtin_offsetof(t, d)=(__builtin_offsetof(t, d) + 0 * sizeof(&((t *)0)->d))",
        );
        command.args(&unit.arguments);
        command.args([
            "-w",
            "-fsyntax-only",
            "-Xclang",
            "-fdump-record-layouts-complete",
            "-Xclang",
            "-ast-dump=json",
        ]);
        if unit.is_cpp {
            command.args(["-x", "c++"]);
        }
        command
    }
}

/// Writes clang's overlay of the export's root: each directory the root
/// holds stands, at its path in the image, for the host's, so that a header
/// an image's source includes by its absolute path is the image's. The
/// directories that hold clang's own headers stay the host's, the root's
/// directories and files inside them laid over one by one. A file read
/// through the overlay keeps its name in the export. Nothing else of the
/// host's is seen: the root and clang's headers stand for themselves, and
/// any other path is not found.
fn write_overlay(root: &Path, resource_dir: &str, to: &Path) -> Result<(), String> {
    fn visit(
        real: &Path,
        virtual_path: &str,
        resource_dir: &str,
        roots: &mut Vec<serde_json::Value>,
    ) -> Result<(), String> {
        let mut entries: Vec<_> = std::fs::read_dir(real)
            .map_err(|e| format!("{}: {e}", real.display()))?
            .filter_map(Result::ok)
            .collect();
        entries.sort_by_key(|entry| entry.file_name());
        for entry in entries {
            let name = entry.file_name().to_string_lossy().into_owned();
            let path = format!("{virtual_path}/{name}");
            let external = entry.path();
            let is_dir = external.is_dir();
            if is_dir && (resource_dir == path || resource_dir.starts_with(&format!("{path}/"))) {
                visit(&external, &path, resource_dir, roots)?;
                continue;
            }
            roots.push(serde_json::json!({
                "type": if is_dir { "directory-remap" } else { "file" },
                "name": path,
                "external-contents": external.to_string_lossy(),
            }));
        }
        Ok(())
    }
    let mut roots = Vec::new();
    for itself in [root.to_string_lossy().into_owned(), resource_dir.to_owned()] {
        roots.push(serde_json::json!({
            "type": "directory-remap",
            "name": itself,
            "external-contents": itself,
        }));
    }
    visit(root, "", resource_dir, &mut roots)?;
    let overlay = serde_json::json!({
        "version": 0,
        "case-sensitive": "true",
        "redirecting-with": "redirect-only",
        "use-external-names": true,
        "roots": roots,
    });
    let text = serde_json::to_string_pretty(&overlay).map_err(|e| e.to_string())?;
    std::fs::write(to, text).map_err(|e| format!("{}: {e}", to.display()))
}

/// clang's errors that leave the dump as the build compiled the unit:
/// a global register variable, which clang allows on the stack pointer
/// alone, is read as the global variable it is.
pub(crate) fn is_tolerated(error: &str) -> bool {
    error.contains("unsuitable for global register variables")
}

/// A dump being read: clang's output as it writes it, its diagnostics
/// collected beside.
pub(crate) struct Dump {
    child: std::process::Child,
    stderr: std::thread::JoinHandle<String>,
}

impl Dump {
    pub(crate) fn start(
        mut command: Command,
    ) -> Result<(Dump, BufReader<std::process::ChildStdout>), String> {
        command
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .stdin(Stdio::null());
        let mut child = command.spawn().map_err(|e| format!("clang: {e}"))?;
        let stdout = child.stdout.take().expect("piped above");
        let mut stderr = child.stderr.take().expect("piped above");
        let stderr = std::thread::spawn(move || {
            let mut text = String::new();
            let _ = stderr.read_to_string(&mut text);
            text
        });
        Ok((
            Dump { child, stderr },
            BufReader::with_capacity(1 << 20, stdout),
        ))
    }

    /// Waits for clang: whether it succeeded, and what it wrote to stderr.
    pub(crate) fn finish(mut self) -> (bool, String) {
        let status = self.child.wait().map(|s| s.success()).unwrap_or(false);
        let stderr = self.stderr.join().unwrap_or_default();
        (status, stderr)
    }
}
