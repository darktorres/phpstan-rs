//! The gate's store: `store.git` in the gate's directory, a git repository
//! of its own in the SHA-256 object format, fed by `git fast-import`.
//!
//! A commit's snapshot is a commit of the store under
//! `refs/snapshots/<hash>`, `<hash>` the full hash of the commit whose build
//! took it. Its tree has an entry for each artifact recorded: a text kept
//! whole is a blob, and a dump (`dumps`), or a text cut as the dumps are
//! (`text_kind`), is its tree, each entity a blob whose id the run computed
//! as it wrote. A pass adds each artifact once its runs are done, as a
//! commit on `refs/pass/<hash>`, which starts from the snapshot `<hash>`
//! has; once every run has passed, the snapshot becomes a commit of that
//! tree over the one before.
//!
//! The store's commands read no configuration but the store's own, so that
//! a diff reads the same on every machine.

use std::collections::{BTreeMap, BTreeSet};
use std::io::{self, BufRead, BufReader, BufWriter, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

use dumps::{Dump, Entry, Id, Kind, Manifest, Mode, Tree, TreeEntry};
use rustc_hash::{FxHashMap, FxHashSet};

use super::gate_dir;

/// How the store keeps the text named `name`: cut as the dumps of the kind
/// given, or whole.
pub(crate) fn text_kind(name: &str) -> Option<Kind> {
    let stem = name.strip_suffix(".txt")?;
    if stem.starts_with("registry-")
        || stem.starts_with("session-")
        || stem.ends_with(".registry")
        || stem == "stub-bases"
    {
        Some(Kind::Tree)
    } else if stem.starts_with("types-")
        || stem.starts_with("expr-types-")
        || stem.starts_with("doc-resolutions-")
    {
        Some(Kind::ByPrefix(b':'))
    } else if matches!(stem, "equalities" | "script" | "verdict") || stem.starts_with("type-drift-")
    {
        None
    } else {
        // The reports, the baselines' and the vendor projects', and every
        // listing of diagnostics: a line each, its path first.
        Some(Kind::ByPrefix(b'\t'))
    }
}

/// The kind of the dump the entry `name` of a snapshot holds, when it holds
/// one.
pub(crate) fn dump_kind(name: &str) -> Option<Kind> {
    if name.starts_with("front-end-") {
        Some(Kind::Files)
    } else {
        text_kind(name)
    }
}

/// What the store keeps of the text `name`, whose bytes are `text`: cut, or
/// whole (`text_kind`).
pub(crate) fn text_content(name: &str, text: Vec<u8>) -> Content {
    match text_kind(name) {
        Some(kind) => Content::Cut(kind, Box::new(std::io::Cursor::new(text))),
        None => Content::Text(text),
    }
}

/// What an entry of a snapshot is made from.
pub(crate) enum Content {
    /// A text kept whole.
    Text(Vec<u8>),
    /// A text cut as the dumps of the kind are, read as it is cut.
    Cut(Kind, Box<dyn Read>),
    /// The dump a run wrote, by its manifest.
    Dump(PathBuf),
    /// A directory of entries by name.
    Dir(Vec<(String, Content)>),
}

pub(crate) struct Store {
    dir: PathBuf,
    /// `git cat-file --batch-check`: whether an object is there.
    check: Option<Batch>,
    /// `git cat-file --batch`: an object's content.
    read: Option<Batch>,
    /// The objects known to be in the store.
    present: FxHashSet<Id>,
}

/// Each snapshot's commit, with the names of its entries.
pub(crate) type Snapshots = BTreeMap<String, BTreeSet<String>>;

/// The ref of the snapshot of `hash`.
pub(crate) fn snapshot_ref(hash: &str) -> String {
    format!("refs/snapshots/{hash}")
}

/// The ref a pass of `hash` adds its artifacts to.
pub(crate) fn pass_ref(hash: &str) -> String {
    format!("refs/pass/{hash}")
}

impl Store {
    /// The gate's store, made when there is none.
    pub(crate) fn open() -> Result<Store, String> {
        let dir = gate_dir().join("store.git");
        if !dir.join("HEAD").is_file() {
            std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
            let store = Store::at(dir.clone());
            run(store
                .git()
                .args(["init", "--bare", "--quiet", "--object-format=sha256"]))?;
        }
        Ok(Store::at(dir))
    }

    fn at(dir: PathBuf) -> Store {
        Store {
            dir,
            check: None,
            read: None,
            present: FxHashSet::default(),
        }
    }

    /// A git command on the store.
    pub(crate) fn git(&self) -> Command {
        let mut cmd = Command::new("git");
        cmd.arg("--git-dir")
            .arg(&self.dir)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_AUTHOR_NAME", "xtask")
            .env("GIT_AUTHOR_EMAIL", "xtask@phpstan-rs")
            .env("GIT_COMMITTER_NAME", "xtask")
            .env("GIT_COMMITTER_EMAIL", "xtask@phpstan-rs")
            .env_remove("GIT_DIR")
            .env_remove("GIT_INDEX_FILE")
            .env_remove("GIT_WORK_TREE");
        cmd
    }

    /// The object `rev` names, when there is one.
    pub(crate) fn resolve(&self, rev: &str) -> Result<Option<Id>, String> {
        let out = self
            .git()
            .args(["rev-parse", "--verify", "--quiet", "--end-of-options"])
            .arg(rev)
            .stderr(Stdio::null())
            .output()
            .map_err(|e| format!("git rev-parse {rev}: {e}"))?;
        if !out.status.success() {
            return Ok(None);
        }
        let id = String::from_utf8_lossy(&out.stdout);
        id.trim()
            .parse()
            .map(Some)
            .map_err(|e| format!("git rev-parse {rev}: {e}"))
    }

    /// Whether the store has the object `id`.
    fn has(&mut self, id: Id) -> Result<bool, String> {
        if self.present.contains(&id) {
            return Ok(true);
        }
        if self.check.is_none() {
            self.check = Some(Batch::start(self.git(), "--batch-check")?);
        }
        let check = self.check.as_mut().expect("started above");
        let found = !check.ask(&id.to_string())?.ends_with(" missing");
        if found {
            self.present.insert(id);
        }
        Ok(found)
    }

    /// The type and content of the object `name` names, when there is one.
    pub(crate) fn object(&mut self, name: &str) -> Result<Option<(String, Vec<u8>)>, String> {
        if self.read.is_none() {
            self.read = Some(Batch::start(self.git(), "--batch")?);
        }
        let read = self.read.as_mut().expect("started above");
        let header = read.ask(name)?;
        let mut fields = header.split(' ');
        let (Some(_), Some(kind), Some(size), None) =
            (fields.next(), fields.next(), fields.next(), fields.next())
        else {
            return Ok(None);
        };
        let size: usize = size
            .parse()
            .map_err(|_| format!("git cat-file: {header}"))?;
        let mut content = vec![0; size + 1];
        read.output
            .read_exact(&mut content)
            .map_err(|e| format!("git cat-file {name}: {e}"))?;
        content.pop();
        Ok(Some((kind.to_string(), content)))
    }

    /// The text of the blob `id`.
    pub(crate) fn blob(&mut self, id: Id) -> Result<Vec<u8>, String> {
        match self.object(&id.to_string())? {
            Some((kind, content)) if kind == "blob" => Ok(content),
            _ => Err(format!("the store has no blob {id}")),
        }
    }

    /// Writes the text the entry `rev` (`<commit>:<path>`) holds to `out`:
    /// a blob's, or a tree's entities' in the order its `.order` lists
    /// them. False when there is no such entry.
    pub(crate) fn write_text(&mut self, rev: &str, out: &mut dyn Write) -> Result<bool, String> {
        match self.object(rev)? {
            None => Ok(false),
            Some((kind, content)) if kind == "blob" => {
                out.write_all(&content).map_err(|e| e.to_string())?;
                Ok(true)
            }
            Some((kind, content)) if kind == "tree" => {
                self.write_tree_text(rev, &content, out)?;
                Ok(true)
            }
            Some((kind, _)) => Err(format!("{rev} is a {kind}, not a text")),
        }
    }

    fn write_tree_text(
        &mut self,
        path: &str,
        content: &[u8],
        out: &mut dyn Write,
    ) -> Result<(), String> {
        let entries = parse_tree(content).map_err(|e| format!("{path}: {e}"))?;
        let order = entries
            .iter()
            .find(|entry| entry.name == dumps::ORDER)
            .ok_or_else(|| format!("{path} is a tree with no {}, not a text", dumps::ORDER))?;
        let order = self.blob(order.id)?;
        let order = String::from_utf8(order).map_err(|e| format!("{path}: {e}"))?;
        let names: Vec<&str> = order.lines().collect();
        if names.len() + 1 != entries.len() {
            return Err(format!(
                "{path}: {} lists {} of {} entries",
                dumps::ORDER,
                names.len(),
                entries.len() - 1
            ));
        }
        for name in names {
            let entry = entries
                .iter()
                .find(|entry| entry.name == name)
                .ok_or_else(|| format!("{path}: {} lists {name}, not there", dumps::ORDER))?;
            let child = format!("{path}/{name}");
            match entry.mode {
                Mode::Blob => out
                    .write_all(&self.blob(entry.id)?)
                    .map_err(|e| e.to_string())?,
                Mode::Tree => {
                    let (_, content) = self
                        .object(&entry.id.to_string())?
                        .ok_or_else(|| format!("{child}: the store has no tree {}", entry.id))?;
                    self.write_tree_text(&child, &content, out)?;
                }
            }
        }
        Ok(())
    }

    /// Each snapshot's commit hash, with the names of its entries.
    pub(crate) fn snapshots(&self) -> Result<Snapshots, String> {
        let refs =
            capture(
                self.git()
                    .args(["for-each-ref", "--format=%(refname)", "refs/snapshots/"]),
            )?;
        let mut snapshots = Snapshots::new();
        for name in refs.lines() {
            let Some(hash) = name.strip_prefix("refs/snapshots/") else {
                continue;
            };
            let listing = capture(self.git().args(["ls-tree", "--name-only", "-z", name]))?;
            let entries = listing
                .split('\0')
                .filter(|entry| !entry.is_empty())
                .map(str::to_string)
                .collect();
            snapshots.insert(hash.to_string(), entries);
        }
        Ok(snapshots)
    }

    /// Writes to `to` the manifest of the dump of `kind` at `rev`
    /// (`<commit>:<path>`): its entities in git's order, none with a text,
    /// for a run to write only the entities that differ from it.
    pub(crate) fn export(&self, rev: &str, kind: Kind, to: &Path) -> Result<(), String> {
        let root = self
            .resolve(rev)?
            .ok_or_else(|| format!("the store has no {rev}"))?;
        let listing = capture(self.git().args(["ls-tree", "-r", "-l", "-z", rev]))?;
        let mut entries = Vec::new();
        for line in listing.split('\0').filter(|line| !line.is_empty()) {
            let (fields, path) = line
                .split_once('\t')
                .ok_or_else(|| format!("git ls-tree {rev}: {line:?}"))?;
            if path.rsplit('/').next() == Some(dumps::ORDER) {
                continue;
            }
            let fields: Vec<&str> = fields.split_whitespace().collect();
            let [_, "blob", id, len] = fields[..] else {
                return Err(format!("git ls-tree {rev}: {line:?}"));
            };
            entries.push(Entry {
                id: id.parse().map_err(|e| format!("{rev}: {e}"))?,
                len: len.parse().map_err(|_| format!("{rev}: {line:?}"))?,
                offset: None,
                path: path.to_string(),
            });
        }
        Manifest {
            kind,
            root,
            reference: None,
            entries,
        }
        .write(to)
        .map_err(|e| format!("{}: {e}", to.display()))
    }

    /// Adds `entries` to the tree of `branch`, each replacing what was
    /// there under its name, in one commit, and requires each entry's tree
    /// or blob in the store to be the one computed for it.
    pub(crate) fn add(
        &mut self,
        branch: &str,
        message: &str,
        entries: Vec<(String, Content)>,
    ) -> Result<(), String> {
        let parent = self.resolve(branch)?;
        let mut import = Import::start(self.git())?;
        let result = self.add_through(&mut import, branch, message, parent, entries);
        let result = result.and_then(|expected| import.finish().map(|()| expected));
        let expected = match result {
            Ok(expected) => expected,
            Err(error) => {
                // What the import sent is not in the store.
                self.present.clear();
                return Err(error);
            }
        };
        for (name, id) in expected {
            let found = self.resolve(&format!("{branch}:{name}"))?;
            if found != Some(id) {
                return Err(format!(
                    "{branch}:{name} is {found:?} in the store, where its entities make {id}"
                ));
            }
        }
        Ok(())
    }

    fn add_through(
        &mut self,
        import: &mut Import,
        branch: &str,
        message: &str,
        parent: Option<Id>,
        entries: Vec<(String, Content)>,
    ) -> Result<Vec<(String, Id)>, String> {
        let mut lines = Vec::new();
        let mut expected = Vec::new();
        for (name, content) in entries {
            let (id, _) = self.put(import, &name, content, &mut lines)?;
            expected.push((name, id));
        }
        let names: Vec<&str> = expected.iter().map(|(name, _)| name.as_str()).collect();
        import
            .commit(branch, message, parent, &names, &lines)
            .map_err(|e| format!("git fast-import: {e}"))?;
        Ok(expected)
    }

    /// Sends what `content` needs that the store lacks, and the lines that
    /// put it at `path`; its id.
    fn put(
        &mut self,
        import: &mut Import,
        path: &str,
        content: Content,
        lines: &mut Vec<String>,
    ) -> Result<(Id, Mode), String> {
        match content {
            Content::Text(text) => {
                let id = Id::blob(&text);
                if !self.has(id)? {
                    self.send(import, id, &text)?;
                }
                lines.push(format!("M 100644 {id} {path}"));
                Ok((id, Mode::Blob))
            }
            Content::Cut(kind, mut text) => {
                let manifest = dumps::cut(kind, &mut text, &mut |_, id, entity| {
                    if !self.has(id).map_err(io::Error::other)? {
                        self.send(import, id, entity).map_err(io::Error::other)?;
                    }
                    Ok(())
                })
                .map_err(|e| format!("{path}: {e}"))?;
                let id = self.put_tree(
                    import,
                    path,
                    &manifest,
                    &mut |_| Err("an entity cut is sent as it comes".to_string()),
                    lines,
                )?;
                Ok((id, Mode::Tree))
            }
            Content::Dump(manifest) => {
                let mut dump = Dump::open(&manifest).map_err(|e| e.to_string())?;
                let kind = dump_kind(path.rsplit('/').next().unwrap_or(path));
                if kind != Some(dump.manifest.kind) {
                    return Err(format!(
                        "{}: a dump of kind {:?} where the store keeps {path} as {kind:?}",
                        manifest.display(),
                        dump.manifest.kind
                    ));
                }
                let manifest = dump.manifest.clone();
                let id = self.put_tree(
                    import,
                    path,
                    &manifest,
                    &mut |index| dump.text(index).map_err(|e| format!("{path}: {e}")),
                    lines,
                )?;
                Ok((id, Mode::Tree))
            }
            Content::Dir(entries) => {
                if entries.is_empty() {
                    return Err(format!("{path}: an empty directory"));
                }
                let mut tree = Vec::new();
                for (name, content) in entries {
                    let (id, mode) = self.put(import, &format!("{path}/{name}"), content, lines)?;
                    tree.push(TreeEntry { name, mode, id });
                }
                Ok((dumps::tree_id(&mut tree), Mode::Tree))
            }
        }
    }

    /// Sends the blobs of `manifest`'s tree the store lacks, taking each
    /// entity's text from `text` by its index, and the lines that put the
    /// tree at `prefix`: a directory the store has whole by its id, the
    /// others entry by entry. Its id.
    fn put_tree(
        &mut self,
        import: &mut Import,
        prefix: &str,
        manifest: &Manifest,
        text: &mut dyn FnMut(usize) -> Result<Vec<u8>, String>,
        lines: &mut Vec<String>,
    ) -> Result<Id, String> {
        if self.has(manifest.root)? {
            lines.push(format!("M 040000 {} {prefix}", manifest.root));
            return Ok(manifest.root);
        }
        let trees = dumps::trees(manifest).map_err(|e| format!("{prefix}: {e}"))?;
        let by_path: FxHashMap<&str, &Tree> = trees
            .iter()
            .map(|tree| (tree.path.as_str(), tree))
            .collect();
        let index: FxHashMap<&str, usize> = manifest
            .entries
            .iter()
            .enumerate()
            .map(|(index, entry)| (entry.path.as_str(), index))
            .collect();
        let mut pending = vec![by_path[""]];
        while let Some(tree) = pending.pop() {
            for entry in &tree.entries {
                let path = dumps::join(&tree.path, &entry.name);
                let at = format!("{prefix}/{path}");
                match entry.mode {
                    Mode::Tree => {
                        let below = by_path[path.as_str()];
                        if self.has(below.id)? {
                            lines.push(format!("M 040000 {} {at}", below.id));
                        } else {
                            pending.push(below);
                        }
                    }
                    Mode::Blob => {
                        if !self.has(entry.id)? {
                            let blob = match &tree.order {
                                Some(order) if entry.name == dumps::ORDER => order.clone(),
                                _ => text(index[path.as_str()])?,
                            };
                            if Id::blob(&blob) != entry.id {
                                return Err(format!("{at}: a text whose id is not {}", entry.id));
                            }
                            self.send(import, entry.id, &blob)?;
                        }
                        lines.push(format!("M 100644 {} {at}", entry.id));
                    }
                }
            }
        }
        Ok(manifest.root)
    }

    fn send(&mut self, import: &mut Import, id: Id, text: &[u8]) -> Result<(), String> {
        import
            .blob(text)
            .map_err(|e| format!("git fast-import: {e}"))?;
        self.present.insert(id);
        Ok(())
    }

    /// Starts a pass of `hash`: its artifacts go on the snapshot `hash`
    /// has, if any.
    pub(crate) fn begin_pass(&self, hash: &str) -> Result<(), String> {
        let pass = pass_ref(hash);
        run(self.git().args(["update-ref", "-d", &pass]))?;
        if let Some(snapshot) = self.resolve(&snapshot_ref(hash))? {
            run(self
                .git()
                .args(["update-ref", &pass, &snapshot.to_string()]))?;
        }
        Ok(())
    }

    /// Makes the pass of `hash` its snapshot: a commit of the pass's tree
    /// over the snapshot before.
    pub(crate) fn finish_pass(&self, hash: &str) -> Result<(), String> {
        let pass = pass_ref(hash);
        let Some(tree) = self.resolve(&format!("{pass}^{{tree}}"))? else {
            return Ok(());
        };
        let snapshot = snapshot_ref(hash);
        let parent = self.resolve(&snapshot)?;
        let mut cmd = self.git();
        cmd.args(["commit-tree", &tree.to_string(), "-m"])
            .arg(format!("the snapshot of {hash}"));
        if let Some(parent) = parent {
            cmd.arg("-p").arg(parent.to_string());
        }
        let commit = capture(&mut cmd)?;
        let mut update = self.git();
        update.args(["update-ref", &snapshot, commit.trim()]);
        if let Some(parent) = parent {
            update.arg(parent.to_string());
        }
        run(&mut update)?;
        run(self.git().args(["update-ref", "-d", &pass]))
    }

    /// Moves the snapshot of `old` to `new`.
    pub(crate) fn rename_snapshot(&self, old: &str, new: &str) -> Result<bool, String> {
        let Some(snapshot) = self.resolve(&snapshot_ref(old))? else {
            return Ok(false);
        };
        run(self
            .git()
            .args(["update-ref", &snapshot_ref(new), &snapshot.to_string()]))?;
        run(self.git().args(["update-ref", "-d", &snapshot_ref(old)]))?;
        Ok(true)
    }

    /// `git diff` of the entry `entry` from the snapshot of `old` to that
    /// of `new`: empty where they are the same.
    pub(crate) fn diff(&self, old: &str, new: &str, entry: &str) -> Result<Vec<u8>, String> {
        let mut cmd = self.git();
        cmd.args([
            "diff",
            "--no-renames",
            "--no-color",
            "--no-ext-diff",
            "--no-textconv",
            &snapshot_ref(old),
            &snapshot_ref(new),
            "--",
            entry,
        ]);
        let shown = format!("{cmd:?}");
        let out = cmd
            .stderr(Stdio::inherit())
            .output()
            .map_err(|e| format!("{shown}: {e}"))?;
        if !out.status.success() {
            return Err(format!("{shown} failed: {}", out.status));
        }
        Ok(out.stdout)
    }
}

/// A `git cat-file` answering one object at a time.
struct Batch {
    child: Child,
    input: ChildStdin,
    output: BufReader<ChildStdout>,
}

impl Batch {
    fn start(mut cmd: Command, mode: &str) -> Result<Batch, String> {
        let mut child = cmd
            .args(["cat-file", mode])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .map_err(|e| format!("git cat-file {mode}: {e}"))?;
        let input = child.stdin.take().expect("piped");
        let output = BufReader::with_capacity(1 << 20, child.stdout.take().expect("piped"));
        Ok(Batch {
            child,
            input,
            output,
        })
    }

    /// Asks for the object `name`; the line that answers.
    fn ask(&mut self, name: &str) -> Result<String, String> {
        let error = |e: io::Error| format!("git cat-file {name}: {e}");
        writeln!(self.input, "{name}").map_err(error)?;
        self.input.flush().map_err(error)?;
        let mut line = String::new();
        if self.output.read_line(&mut line).map_err(error)? == 0 {
            return Err(format!("git cat-file {name}: it ended"));
        }
        line.pop();
        Ok(line)
    }
}

impl Drop for Batch {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// A `git fast-import` taking one commit.
struct Import {
    child: Child,
    input: Option<BufWriter<ChildStdin>>,
}

impl Import {
    fn start(mut cmd: Command) -> Result<Import, String> {
        let mut child = cmd
            .args(["fast-import", "--quiet", "--done"])
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .spawn()
            .map_err(|e| format!("git fast-import: {e}"))?;
        let input = BufWriter::with_capacity(1 << 20, child.stdin.take().expect("piped"));
        Ok(Import {
            child,
            input: Some(input),
        })
    }

    fn input(&mut self) -> io::Result<&mut BufWriter<ChildStdin>> {
        self.input
            .as_mut()
            .ok_or_else(|| io::Error::other("the import is finished"))
    }

    fn blob(&mut self, text: &[u8]) -> io::Result<()> {
        let input = self.input()?;
        write!(input, "blob\ndata {}\n", text.len())?;
        input.write_all(text)?;
        input.write_all(b"\n")
    }

    /// The commit on `branch` over `parent` that replaces the entries
    /// `names` with what `lines` put.
    fn commit(
        &mut self,
        branch: &str,
        message: &str,
        parent: Option<Id>,
        names: &[&str],
        lines: &[String],
    ) -> io::Result<()> {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |since| since.as_secs());
        let input = self.input()?;
        write!(
            input,
            "commit {branch}\ncommitter xtask <xtask@phpstan-rs> {now} +0000\ndata {}\n{message}\n",
            message.len()
        )?;
        if let Some(parent) = parent {
            writeln!(input, "from {parent}")?;
        }
        for name in names {
            writeln!(input, "D {name}")?;
        }
        for line in lines {
            writeln!(input, "{line}")?;
        }
        writeln!(input)
    }

    fn finish(mut self) -> Result<(), String> {
        let mut input = self.input.take().expect("not finished");
        input
            .write_all(b"done\n")
            .and_then(|()| input.flush())
            .map_err(|e| format!("git fast-import: {e}"))?;
        drop(input);
        let status = self
            .child
            .wait()
            .map_err(|e| format!("git fast-import: {e}"))?;
        if status.success() {
            Ok(())
        } else {
            Err(format!("git fast-import failed: {status}"))
        }
    }
}

impl Drop for Import {
    fn drop(&mut self) {
        if self.input.take().is_some() {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }
}

/// The entries of a tree object.
fn parse_tree(mut content: &[u8]) -> Result<Vec<TreeEntry>, String> {
    let mut entries = Vec::new();
    while !content.is_empty() {
        let space = memchr(b' ', content).ok_or("a tree entry with no mode")?;
        let mode = match &content[..space] {
            b"40000" => Mode::Tree,
            b"100644" => Mode::Blob,
            mode => {
                return Err(format!(
                    "a tree entry of mode {}",
                    String::from_utf8_lossy(mode)
                ));
            }
        };
        content = &content[space + 1..];
        let nul = memchr(0, content).ok_or("a tree entry with no name")?;
        let name = String::from_utf8(content[..nul].to_vec()).map_err(|e| e.to_string())?;
        content = &content[nul + 1..];
        let (id, rest) = content
            .split_first_chunk::<32>()
            .ok_or("a tree entry with no id")?;
        content = rest;
        entries.push(TreeEntry {
            name,
            mode,
            id: Id(*id),
        });
    }
    Ok(entries)
}

fn memchr(byte: u8, bytes: &[u8]) -> Option<usize> {
    bytes.iter().position(|&b| b == byte)
}

fn run(cmd: &mut Command) -> Result<(), String> {
    super::run(cmd.stdout(Stdio::null()))
}

fn capture(cmd: &mut Command) -> Result<String, String> {
    super::capture(cmd)
}
