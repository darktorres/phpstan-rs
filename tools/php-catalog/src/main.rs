//! The PHP catalog: what PHP's builtin functions, methods and opcode
//! handlers may do, read from the source of a PHP build exported from its
//! image (`tools/php-images`).

mod ast;
mod cfg;
mod clang;
mod ir;
mod layout;
mod lower;
mod print;
mod program;
mod registry;

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::time::Instant;

use rustc_hash::{FxHashMap, FxHashSet};

use crate::ast::{Kind, Node, TopLevel};
use crate::clang::{Dump, Export, Platform, Unit};
use crate::ir::Function;
use crate::lower::{TuIndex, catalog_path, has_body, is_static, lower_function, lower_global};
use crate::program::{Program, UnitPart};

/// Each worker's stack: the dump's trees are read and lowered recursively.
const STACK_SIZE: usize = 1 << 30;

struct Options {
    export: PathBuf,
    jobs: usize,
    platform: Platform,
    /// Only the units whose file contains one of these.
    only: Vec<String>,
    /// The functions whose lowering is printed.
    print: Vec<String>,
    /// Print each unit's clang command, and nothing else.
    commands: bool,
    /// Where the catalog's files are written.
    out: Option<PathBuf>,
}

fn usage() -> String {
    "usage: php-catalog <export dir> [--jobs <n>] [--windows] [--only <file part>]... \
     [--print <function>]... [--command] [--out <dir>]"
        .to_owned()
}

fn options() -> Result<Options, String> {
    let mut args = std::env::args().skip(1);
    let mut export = None;
    let mut jobs = std::thread::available_parallelism().map_or(1, |n| n.get());
    let mut platform = Platform::Linux;
    let mut only = Vec::new();
    let mut print = Vec::new();
    let mut commands = false;
    let mut out = None;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--jobs" => {
                jobs = args.next().and_then(|n| n.parse().ok()).ok_or_else(usage)?;
            }
            "--windows" => platform = Platform::Windows,
            "--only" => only.push(args.next().ok_or_else(usage)?),
            "--print" => print.push(args.next().ok_or_else(usage)?),
            "--command" => commands = true,
            "--out" => out = Some(PathBuf::from(args.next().ok_or_else(usage)?)),
            _ if arg.starts_with('-') => return Err(usage()),
            _ => export = Some(PathBuf::from(arg)),
        }
    }
    Ok(Options {
        export: export.ok_or_else(usage)?,
        jobs: jobs.max(1),
        platform,
        only,
        print,
        commands,
        out,
    })
}

/// The top-level declarations already read, by where they are: a
/// declaration a header gives every unit is read from one.
#[derive(Default)]
struct Claims {
    taken: Mutex<FxHashSet<(Arc<str>, u32)>>,
}

impl Claims {
    fn claim(&self, file: Arc<str>, offset: u32) -> bool {
        self.taken
            .lock()
            .expect("no worker panics holding it")
            .insert((file, offset))
    }
}

/// Keeps a unit's declarations: each function and variable with its
/// children where this unit claims it, each typedef, record and
/// enumeration with its children.
struct Collector<'c> {
    claims: &'c Claims,
    root: &'c str,
    paths: FxHashMap<Arc<str>, Arc<str>>,
    nodes: Vec<Node>,
}

impl TopLevel for Collector<'_> {
    fn wants(&mut self, node: &Node) -> bool {
        match node.kind {
            Kind::FunctionDecl | Kind::VarDecl => {
                let Some(file) = &node.loc.file else {
                    return true;
                };
                let path = match self.paths.get(file) {
                    Some(path) => path.clone(),
                    None => {
                        let path = catalog_path(self.root, file);
                        self.paths.insert(file.clone(), path.clone());
                        path
                    }
                };
                self.claims.claim(path, node.loc.offset)
            }
            Kind::RecordDecl | Kind::EnumDecl => true,
            _ => false,
        }
    }

    fn take(&mut self, node: Node) {
        if matches!(
            node.kind,
            Kind::FunctionDecl
                | Kind::VarDecl
                | Kind::RecordDecl
                | Kind::EnumDecl
                | Kind::TypedefDecl
        ) {
            self.nodes.push(node);
        }
    }
}

/// What reading a unit gave.
struct UnitResult {
    file: String,
    /// Whether clang compiled the unit without errors.
    compiled: bool,
    /// Why the unit was not read, where it was not.
    failure: Option<String>,
    /// clang's first errors.
    errors: Vec<String>,
    part: UnitPart,
    seconds: f64,
}

fn read_unit(export: &Export, unit: &Unit, claims: &Claims, platform: Platform) -> UnitResult {
    let start = Instant::now();
    let mut result = UnitResult {
        file: unit.file.clone(),
        compiled: false,
        failure: None,
        errors: Vec::new(),
        part: UnitPart::default(),
        seconds: 0.0,
    };
    if unit.is_cpp {
        result.failure = Some("C++".to_owned());
        return result;
    }
    let (dump, mut reader) = match Dump::start(export.command(unit, platform)) {
        Ok(started) => started,
        Err(e) => {
            result.failure = Some(e);
            return result;
        }
    };
    let root = export.root_prefix();
    let mut collector = Collector {
        claims,
        root: &root,
        paths: FxHashMap::default(),
        nodes: Vec::new(),
    };
    let root_text = export.root_prefix();
    let layouts = match layout::read_layouts(&mut reader, &root_text) {
        Ok(read) => read,
        Err(e) => {
            dump.finish();
            result.failure = Some(format!("reading the layouts: {e}"));
            return result;
        }
    };
    let mut deserializer = serde_json::Deserializer::from_reader(reader);
    deserializer.disable_recursion_limit();
    let read = ast::read_dump(&mut deserializer, &mut collector);
    let (succeeded, stderr) = dump.finish();
    let errors: Vec<&str> = stderr
        .lines()
        .filter(|line| line.contains("error:"))
        .collect();
    let tolerated = errors.iter().filter(|e| clang::is_tolerated(e)).count();
    result.compiled = succeeded || (tolerated > 0 && tolerated == errors.len());
    result.errors = errors
        .into_iter()
        .filter(|e| !clang::is_tolerated(e))
        .take(5)
        .map(str::to_owned)
        .collect();
    if let Err(e) = read {
        result.failure = Some(format!("reading the dump: {e}"));
        return result;
    }
    let mut index = TuIndex::new(Arc::from(root.as_str()), layouts, platform);
    index.index(&collector.nodes);
    for node in &collector.nodes {
        match node.kind {
            Kind::FunctionDecl if has_body(node) => {
                let Some(name) = index.functions.get(&node.id).cloned() else {
                    continue;
                };
                let (function, statics) = lower_function(&mut index, node, name);
                result.part.functions.push(function);
                result.part.globals.extend(statics);
            }
            Kind::FunctionDecl if !is_static(node) => {
                let Some(name) = index.functions.get(&node.id).cloned() else {
                    continue;
                };
                let file = index.node_path(node);
                result.part.declared.push((name, file));
            }
            Kind::VarDecl if node.has_init && !node.inner.is_empty() => {
                let Some(name) = index.globals.get(&node.id).cloned() else {
                    continue;
                };
                result
                    .part
                    .globals
                    .push(lower_global(&mut index, node, name));
            }
            _ => {}
        }
    }
    result.part.layouts = std::mem::take(&mut index.layouts);
    result.part.typedefs = std::mem::take(&mut index.typedefs);
    result.seconds = start.elapsed().as_secs_f64();
    result
}

fn run(options: &Options) -> Result<(), String> {
    let export = Export::open(&options.export)?;
    let units: Vec<Unit> = export
        .units(&options.export.join("compile_commands.json"))?
        .into_iter()
        .filter(|unit| {
            options.only.is_empty() || options.only.iter().any(|o| unit.file.contains(o))
        })
        .collect();
    if options.commands {
        for unit in &units {
            let command = export.command(unit, options.platform);
            let mut line = format!(
                "cd {} &&",
                shell_word(
                    &command
                        .get_current_dir()
                        .unwrap_or(std::path::Path::new("."))
                        .to_string_lossy()
                )
            );
            line.push(' ');
            line.push_str(&shell_word(&command.get_program().to_string_lossy()));
            for arg in command.get_args() {
                line.push(' ');
                line.push_str(&shell_word(&arg.to_string_lossy()));
            }
            println!("{line}");
        }
        return Ok(());
    }
    let claims = Claims::default();
    let next = AtomicUsize::new(0);
    let start = Instant::now();
    let mut report = Report::default();
    let mut program = Program::default();
    std::thread::scope(|scope| {
        let (send, receive) = mpsc::channel();
        for _ in 0..options.jobs.min(units.len().max(1)) {
            let send = send.clone();
            let (units, export, claims, next) = (&units, &export, &claims, &next);
            std::thread::Builder::new()
                .stack_size(STACK_SIZE)
                .spawn_scoped(scope, move || {
                    loop {
                        let i = next.fetch_add(1, Ordering::Relaxed);
                        let Some(unit) = units.get(i) else { break };
                        let result = read_unit(export, unit, claims, options.platform);
                        if send.send(result).is_err() {
                            break;
                        }
                    }
                })
                .expect("spawning a worker");
        }
        drop(send);
        for result in receive {
            report.observe(&result, options);
            program.add(result.part);
        }
    });
    program.finish();
    report.print(&units, &program, start.elapsed().as_secs_f64());
    let registry = registry::find(&program);
    let methods: usize = registry.classes.values().map(|c| c.methods.len()).sum();
    println!(
        "registry: {} functions, {} classes, {methods} methods, {} arrays unregistered",
        registry.functions.len(),
        registry.classes.len(),
        registry.unregistered.len()
    );
    if let Some(out) = &options.out {
        std::fs::create_dir_all(out).map_err(|e| format!("{}: {e}", out.display()))?;
        let path = out.join("registrations.tsv");
        std::fs::write(&path, registry.to_tsv()).map_err(|e| format!("{}: {e}", path.display()))?;
    }
    Ok(())
}

/// What the units read gave, for the report.
#[derive(Default)]
struct Report {
    units: usize,
    functions: usize,
    globals: usize,
    with_unread: usize,
    unread: BTreeMap<Arc<str>, Tally>,
    approximated: BTreeMap<Arc<str>, Tally>,
    /// The units not read, with why.
    failed: Vec<(String, String)>,
    /// The units clang compiled with errors, with the first ones.
    not_compiled: Vec<(String, Vec<String>)>,
    seconds: Vec<(f64, String)>,
}

impl Report {
    fn observe(&mut self, result: &UnitResult, options: &Options) {
        self.units += 1;
        self.functions += result.part.functions.len();
        self.globals += result.part.globals.len();
        if let Some(failure) = &result.failure {
            self.failed.push((result.file.clone(), failure.clone()));
        } else if !result.compiled {
            self.not_compiled
                .push((result.file.clone(), result.errors.clone()));
        }
        self.seconds.push((result.seconds, result.file.clone()));
        for function in &result.part.functions {
            if !function.unread.is_empty() {
                self.with_unread += 1;
            }
            for what in &function.unread {
                self.unread.entry(what.clone()).or_default().add(function);
            }
            for what in &function.approximated {
                self.approximated
                    .entry(what.clone())
                    .or_default()
                    .add(function);
            }
            let bare = function
                .name
                .rsplit_once(':')
                .map_or(&*function.name, |(_, name)| name);
            if options
                .print
                .iter()
                .any(|p| **p == *function.name || p == bare)
            {
                println!("{}", print::function(function));
            }
        }
        for global in &result.part.globals {
            let bare = global
                .name
                .rsplit_once(':')
                .map_or(&*global.name, |(_, name)| name);
            if options
                .print
                .iter()
                .any(|p| **p == *global.name || p == bare)
            {
                println!("{}", print::global(global));
            }
        }
    }

    fn print(&mut self, units: &[Unit], program: &Program, seconds: f64) {
        println!(
            "{} units in {seconds:.1} s: {} functions ({} with something unread), {} globals",
            self.units, self.functions, self.with_unread, self.globals
        );
        println!("not read: {}", self.failed.len());
        let mut reasons: BTreeMap<&str, usize> = BTreeMap::new();
        for (_, why) in &self.failed {
            *reasons.entry(why).or_default() += 1;
        }
        for (why, count) in &reasons {
            println!("  {count:5} {why}");
        }
        println!("compiled with errors: {}", self.not_compiled.len());
        self.not_compiled.sort();
        for (file, errors) in self.not_compiled.iter().take(20) {
            println!("  {file}");
            for error in errors {
                println!("    {error}");
            }
        }
        println!("unread:");
        for (what, tally) in &self.unread {
            tally.print(what);
        }
        println!("approximated:");
        for (what, tally) in &self.approximated {
            tally.print(what);
        }
        let mut dropped: BTreeMap<&str, usize> = BTreeMap::new();
        for unit in units {
            for option in &unit.dropped {
                *dropped.entry(option).or_default() += 1;
            }
        }
        println!("options not passed on:");
        for (option, count) in &dropped {
            println!("  {count:6} {option}");
        }
        let mut defined_twice: Vec<(&str, usize)> = program
            .functions
            .iter()
            .filter(|(_, definitions)| definitions.len() > 1)
            .map(|(name, definitions)| (&**name, definitions.len()))
            .collect();
        defined_twice.sort_unstable();
        println!(
            "program: {} functions, {} globals, {} records, {} functions declared only",
            program.functions.len(),
            program.globals.len(),
            program.records.len(),
            program.declared.len()
        );
        println!("functions defined more than once: {}", defined_twice.len());
        for (name, count) in &defined_twice {
            let files: Vec<&str> = program.functions[*name].iter().map(|f| &*f.file).collect();
            println!("  {count} {name}: {}", files.join(" "));
        }
        let mut typedef_conflicts: Vec<_> = program.typedef_conflicts.iter().collect();
        typedef_conflicts.sort_unstable();
        println!(
            "typedefs given different types: {}",
            typedef_conflicts.len()
        );
        for (name, types) in &typedef_conflicts {
            let types: Vec<&str> = types.iter().map(|t| &**t).collect();
            println!("  {name}: {}", types.join(" | "));
        }
        let mut conflicts: Vec<(&Arc<str>, &Vec<String>)> = program.conflicts.iter().collect();
        conflicts.sort_unstable();
        println!("records laid out differently: {}", conflicts.len());
        for (name, layouts) in &conflicts {
            println!("  {name}: {}", layouts.join(" | "));
        }
        self.seconds.sort_by(|a, b| b.0.total_cmp(&a.0));
        println!("slowest:");
        for (seconds, file) in self.seconds.iter().take(5) {
            println!("  {seconds:6.1} s {file}");
        }
    }
}

/// A word quoted for the shell where it needs it.
fn shell_word(word: &str) -> String {
    if !word.is_empty()
        && word
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"-_./=:,+@%".contains(&b))
    {
        word.to_owned()
    } else {
        format!("'{}'", word.replace('\'', "'\\''"))
    }
}

/// How many functions show something, with the first few of them.
#[derive(Default)]
struct Tally {
    count: usize,
    examples: Vec<String>,
}

impl Tally {
    fn add(&mut self, function: &Function) {
        self.count += 1;
        if self.examples.len() < 3 {
            self.examples.push(format!(
                "{}:{} {}",
                function.file, function.line, function.name
            ));
        }
    }

    fn print(&self, what: &str) {
        println!("  {:6} {what}", self.count);
        for example in &self.examples {
            println!("           {example}");
        }
    }
}

fn main() -> ExitCode {
    let options = match options() {
        Ok(options) => options,
        Err(e) => {
            eprintln!("{e}");
            return ExitCode::from(2);
        }
    };
    match run(&options) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("{e}");
            ExitCode::FAILURE
        }
    }
}
