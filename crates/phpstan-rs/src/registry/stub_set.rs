//! The built-in stubs of a run, as a value its entry point owns and hands
//! down, rather than process-wide state any code can force.
//!
//! A `Stubs` holds three values, each built at most once on a thread of
//! its own: the `Corpus` (the stub index and declarations), and the two
//! registries resolved from it, one with PHPStan's own source and one
//! without. Whoever needs a value waits for it by blocking in the OS, so a
//! waiting thread never takes another job from its pool: a thread of a
//! rayon pool that forced a build while holding a lock could otherwise be
//! handed a job that needs the same lock, and wait on itself. A build's
//! parallel work runs on the pool the entry point gave, from a thread that
//! belongs to no pool.
//!
//! Only the entry points (the `check-project` command, the language
//! server, `incremental-bench`, a test harness) hold a `Stubs`. A
//! `Project` and a `Registry` hold what was built from it, so a pool job
//! cannot start or wait for a build.

use std::sync::{Arc, Condvar, Mutex};

use super::stubs::Corpus;
use super::{CacheDir, Registry};

/// Where a `Stubs` finds the stub data it does not build.
#[derive(Clone, Debug)]
pub enum StubSource {
    /// The stub cache in `dir`: what it holds for this build is read, and
    /// what is built is stored there.
    Cached { dir: Arc<CacheDir> },
    /// Nothing is read or stored: the stubs are built from the embedded
    /// sources.
    Build,
}

impl StubSource {
    /// The stub cache the environment names (`registry::cache_dir`), else
    /// `Build`: `PHPSTAN_RS_NO_STUB_CACHE` turns the cache off.
    pub fn from_environment() -> StubSource {
        match super::cache_dir() {
            Some(dir) => StubSource::Cached {
                dir: Arc::new(CacheDir::new(dir)),
            },
            None => StubSource::Build,
        }
    }

    pub(crate) fn dir(&self) -> Option<&Arc<CacheDir>> {
        match self {
            StubSource::Cached { dir } => Some(dir),
            StubSource::Build => None,
        }
    }
}

/// A value built once on a thread of its own, which any thread may wait
/// for.
struct Built<T>(Arc<Shared<T>>);

struct Shared<T> {
    slot: Mutex<Slot<T>>,
    ready: Condvar,
}

enum Slot<T> {
    Idle,
    Building,
    Done(T),
    Failed(String),
}

impl<T: Clone + Send + 'static> Built<T> {
    fn new() -> Self {
        Built(Arc::new(Shared {
            slot: Mutex::new(Slot::Idle),
            ready: Condvar::new(),
        }))
    }

    /// Starts `build` on a thread named `name` unless it has started.
    fn start(&self, name: &str, build: impl FnOnce() -> T + Send + 'static) {
        let mut slot = self.0.slot.lock().unwrap_or_else(|e| e.into_inner());
        if !matches!(*slot, Slot::Idle) {
            return;
        }
        *slot = Slot::Building;
        let shared = Arc::clone(&self.0);
        let spawned = std::thread::Builder::new()
            .name(name.to_string())
            .stack_size(crate::project::WORKER_STACK_SIZE)
            .spawn(move || {
                let outcome = stack::catch_quietly(build);
                let mut slot = shared.slot.lock().unwrap_or_else(|e| e.into_inner());
                *slot = match outcome {
                    Ok(value) => Slot::Done(value),
                    Err(payload) => Slot::Failed(
                        payload
                            .downcast_ref::<&str>()
                            .map(|message| message.to_string())
                            .or_else(|| payload.downcast_ref::<String>().cloned())
                            .unwrap_or_else(|| "the build panicked".to_string()),
                    ),
                };
                shared.ready.notify_all();
            });
        if let Err(error) = spawned {
            *slot = Slot::Failed(format!("the build thread did not start: {error}"));
            self.0.ready.notify_all();
        }
    }

    /// The built value, once it is. Blocks in the OS: the caller takes no
    /// job from a pool while it waits. A build that panicked panics every
    /// waiter.
    fn wait(&self) -> T {
        let mut slot = self.0.slot.lock().unwrap_or_else(|e| e.into_inner());
        loop {
            match &*slot {
                Slot::Done(value) => return value.clone(),
                Slot::Failed(message) => panic!("building the stubs failed: {message}"),
                Slot::Idle | Slot::Building => {
                    slot = self.0.ready.wait(slot).unwrap_or_else(|e| e.into_inner());
                }
            }
        }
    }
}

struct Inner {
    source: StubSource,
    pool: Arc<rayon::ThreadPool>,
    corpus: Built<Arc<Corpus>>,
    plain: Built<Registry>,
    phpstan: Built<Registry>,
}

/// The built-in stubs of a run (see the module's comment). Cheap to clone;
/// the clones share the built values.
#[derive(Clone)]
pub struct Stubs {
    inner: Arc<Inner>,
}

impl Stubs {
    /// The stubs from `source`, whose builds run on `pool`. Starts the
    /// corpus's build now, on a thread of its own, so that it runs beside
    /// whatever the caller does before it first needs the stubs.
    pub fn new(source: StubSource, pool: Arc<rayon::ThreadPool>) -> Stubs {
        let stubs = Stubs {
            inner: Arc::new(Inner {
                source,
                pool,
                corpus: Built::new(),
                plain: Built::new(),
                phpstan: Built::new(),
            }),
        };
        stubs.start_corpus();
        stubs
    }

    /// The stubs of the environment's stub cache, built on the process's
    /// worker pool, shared by every test that makes a project.
    #[cfg(test)]
    pub fn shared() -> &'static Stubs {
        static SHARED: std::sync::OnceLock<Stubs> = std::sync::OnceLock::new();
        SHARED.get_or_init(|| {
            Stubs::new(
                StubSource::from_environment(),
                crate::project::worker_pool(),
            )
        })
    }

    /// The pool the builds' parallel work runs on.
    pub fn pool(&self) -> &Arc<rayon::ThreadPool> {
        &self.inner.pool
    }

    fn start_corpus(&self) {
        let inner = Arc::clone(&self.inner);
        self.inner.corpus.start("stub-corpus", move || {
            Arc::new(match &inner.source {
                StubSource::Cached { dir } => Corpus::obtain(Some(dir), &inner.pool),
                StubSource::Build => Corpus::built(Some(&inner.pool)),
            })
        });
    }

    /// The stubs' index and declarations, once they are built.
    pub fn corpus(&self) -> Arc<Corpus> {
        self.inner.corpus.wait()
    }

    /// The registry of the built-in stubs, with PHPStan's own source when
    /// `with_phpstan`, resolved and frozen to share, once it is built. The
    /// first ask starts its build.
    pub fn base(&self, with_phpstan: bool) -> Registry {
        let cell = if with_phpstan {
            &self.inner.phpstan
        } else {
            &self.inner.plain
        };
        let inner = Arc::clone(&self.inner);
        cell.start(
            if with_phpstan {
                "stub-base-phpstan"
            } else {
                "stub-base"
            },
            move || {
                let stubs = Stubs { inner };
                crate::project::Project::resolved_stubs(&stubs, with_phpstan)
            },
        );
        cell.wait()
    }

    /// The stub cache's directory, if the stubs have one.
    pub(crate) fn cache_dir(&self) -> Option<&CacheDir> {
        self.inner.source.dir().map(|dir| &**dir)
    }
}
