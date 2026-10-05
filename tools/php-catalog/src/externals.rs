//! What the functions the build calls without their source do with the
//! pointers they are given: the compiler's builtins and the C library,
//! function by function, and php-src's own allocators, whose memory the
//! points-to analysis tells apart by the call allocating it.

/// Where a pointer a call writes or passes on points.
#[derive(Debug, Clone, Copy)]
pub(crate) enum Src {
    /// The argument itself.
    Arg(usize),
    /// Into the argument's memory: at an element where the other argument
    /// is the elements' size, else anywhere in it.
    Into(usize, Option<usize>),
    /// Into the C library's own memory, which holds whatever it keeps.
    Own,
}

/// One thing a call does with pointers.
#[derive(Debug, Clone, Copy)]
pub(crate) enum Effect {
    /// Gives a pointer into the argument's memory.
    Gives(usize),
    /// Gives a pointer the argument's memory holds.
    GivesHeld(usize),
    /// Gives memory it allocates.
    Allocates,
    /// Gives memory it allocates, holding what the argument's memory
    /// held.
    Grows(usize),
    /// Gives memory it allocates, holding bytes from outside the process.
    AllocatesOutside,
    /// Is given back a block it gave, at the argument: what it does with
    /// the block's pointer (its free lists, its headers) is its own, so
    /// the pointer reaches nothing past the call.
    Block(usize),
    /// Gives memory it maps: zeroed where the argument's flags are
    /// constant and ask for an anonymous mapping, else a file's bytes,
    /// from outside the process.
    Maps { flags: usize },
    /// Gives memory of the C library's own.
    GivesOwn,
    /// Gives a value computed from the arguments' values.
    Derives,
    /// Gives a pointer into code loaded while the program runs, which the
    /// build has no source of.
    GivesLoaded,
    /// Copies what one argument's memory holds into another's: as many
    /// bytes as the third argument gives where it is constant, else to
    /// the end of the memory.
    Copies {
        into: usize,
        from: usize,
        len: Option<usize>,
    },
    /// Writes a pointer into one argument's memory through another.
    WritesInto { through: usize, from: usize },
    /// Writes pointers into one argument's memory, anywhere in it, through
    /// another.
    Fills { through: usize, from: usize },
    /// Writes bytes from outside the process into the memory `depth`
    /// pointers away from the argument: through the argument itself at
    /// depth 0, through the pointers its memory holds at depth 1.
    WritesOutside { arg: usize, depth: u8 },
    /// Writes memory it allocates through the argument.
    WritesNew(usize),
    /// Writes memory of the C library's own through the argument.
    WritesOwn(usize),
    /// Keeps the argument in the C library's own memory.
    Holds(usize),
    /// Keeps what the argument's memory holds in the C library's own
    /// memory.
    HoldsHeld(usize),
    /// Writes what the C library's own memory holds into the argument's
    /// memory.
    WritesHeld(usize),
    /// Calls the function an argument gives, now, each of its arguments
    /// a pointer into the memory listed.
    Calls {
        callee: usize,
        args: &'static [&'static [Src]],
    },
    /// Keeps the function an argument gives, which runs later.
    Keeps {
        callee: usize,
        args: &'static [&'static [Src]],
        when: Later,
    },
    /// Keeps the functions the members of a record of the C library's
    /// hold, which run later: the record the argument points to, or is.
    KeepsMembers {
        arg: usize,
        record: &'static str,
        when: Later,
    },
    /// Starts a `va_list` at the argument: the variable arguments of the
    /// function calling it.
    StartsVaList(usize),
    /// Copies a `va_list` into another.
    CopiesVaList { into: usize, from: usize },
    /// Runs the functions the C library keeps for when given, now.
    Runs(Later),
}

/// When code the C library keeps runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub(crate) enum Later {
    /// On a signal, between any two instructions.
    Signal,
    /// At the process's exit.
    Exit,
    /// In a thread of its own.
    Thread,
    /// At a fork, before and after it.
    Fork,
    /// When a thread with a key's value set exits.
    ThreadExit,
    /// In a call of the C library's on a stream it opened over the
    /// functions (`fopencookie`), or of another library given the stream.
    Stream,
}

/// The effects of a call of a function the build has no source of, where
/// the tables know them.
pub(crate) fn effects(name: &str) -> Option<&'static [Effect]> {
    use Effect::*;
    use Src::*;
    if let Some(effects) = builtin(name) {
        return Some(effects);
    }
    Some(match base_name(name) {
        "va_start" => &[StartsVaList(0)],
        "va_copy" => &[CopiesVaList { into: 0, from: 1 }],
        "va_end" => &[],
        // The atomic builtins' arguments are in clang's order: the
        // pointer, the order, then the values.
        "__atomic_load_n" | "__c11_atomic_load" => &[GivesHeld(0)],
        "__atomic_load" => &[Copies {
            into: 2,
            from: 0,
            len: None,
        }],
        "__atomic_store_n" | "__c11_atomic_store" => &[WritesInto {
            through: 0,
            from: 2,
        }],
        "__atomic_store" => &[Copies {
            into: 0,
            from: 2,
            len: None,
        }],
        "__atomic_exchange" => &[
            Copies {
                into: 3,
                from: 0,
                len: None,
            },
            Copies {
                into: 0,
                from: 2,
                len: None,
            },
        ],
        "__atomic_compare_exchange_n"
        | "__c11_atomic_compare_exchange_strong"
        | "__c11_atomic_compare_exchange_weak" => &[
            WritesInto {
                through: 0,
                from: 4,
            },
            Copies {
                into: 2,
                from: 0,
                len: None,
            },
        ],
        "__atomic_compare_exchange" => &[
            Copies {
                into: 0,
                from: 4,
                len: None,
            },
            Copies {
                into: 2,
                from: 0,
                len: None,
            },
        ],
        "__atomic_test_and_set"
        | "__atomic_clear"
        | "__atomic_thread_fence"
        | "__atomic_signal_fence"
        | "__c11_atomic_thread_fence"
        | "__c11_atomic_signal_fence"
        | "__sync_synchronize"
        | "__sync_lock_release" => &[],
        "__sync_val_compare_and_swap" => &[
            GivesHeld(0),
            WritesInto {
                through: 0,
                from: 2,
            },
        ],
        "__sync_bool_compare_and_swap" => &[WritesInto {
            through: 0,
            from: 2,
        }],
        atomic if atomic.starts_with("__atomic_") || atomic.starts_with("__c11_atomic_") => &[
            GivesHeld(0),
            WritesInto {
                through: 0,
                from: 2,
            },
        ],
        sync if sync.starts_with("__sync_") => &[
            GivesHeld(0),
            WritesInto {
                through: 0,
                from: 1,
            },
        ],
        "memcpy" | "memmove" | "mempcpy" => &[
            Copies {
                into: 0,
                from: 1,
                len: Some(2),
            },
            Gives(0),
        ],
        "wmemcpy" | "wmemmove" | "wmempcpy" => &[
            Copies {
                into: 0,
                from: 1,
                len: None,
            },
            Gives(0),
        ],
        "bcopy" => &[Copies {
            into: 1,
            from: 0,
            len: Some(2),
        }],
        "malloc" | "calloc" | "aligned_alloc" | "memalign" | "valloc" | "pvalloc" | "alloca"
        | "alloca_with_align" | "strdup" | "strndup" | "wcsdup" | "tempnam"
        | "__sched_cpualloc" => &[Allocates],
        "mmap" | "mmap64" => &[Maps { flags: 3 }],
        "shmat" | "backtrace_symbols" => &[AllocatesOutside],
        "realloc" | "reallocarray" | "mremap" => &[Grows(0)],
        "posix_memalign" | "asprintf" | "vasprintf" => &[WritesNew(0)],
        "getifaddrs" => &[WritesNew(0), WritesOutside { arg: 0, depth: 1 }],
        "getline" | "getdelim" => &[
            WritesNew(0),
            WritesOutside { arg: 0, depth: 1 },
            Runs(Later::Stream),
        ],
        "realpath" => &[
            AllocatesOutside,
            Gives(1),
            WritesOutside { arg: 1, depth: 0 },
        ],
        "getcwd" => &[
            AllocatesOutside,
            Gives(0),
            WritesOutside { arg: 0, depth: 0 },
        ],
        "read" | "pread" | "pread64" | "recv" | "getrandom" | "getentropy" | "arc4random_buf"
        | "gethostname" | "getdomainname" | "uname" | "getlogin_r" | "pipe" | "pipe2" | "time"
        | "gettimeofday" | "times" | "sysinfo" | "poll" | "ppoll" | "getgroups" => {
            &[WritesOutside { arg: 0, depth: 0 }]
        }
        "stat" | "stat64" | "lstat" | "lstat64" | "fstat" | "fstat64" | "statvfs" | "statvfs64"
        | "fstatvfs" | "fstatvfs64" | "statfs" | "fstatfs" | "readlink" | "clock_gettime"
        | "getrusage" | "getrlimit" | "getrlimit64" | "getitimer" | "ttyname_r" | "ptsname_r"
        | "tcgetattr" | "sigpending" | "getresuid" | "getresgid" => {
            &[WritesOutside { arg: 1, depth: 0 }]
        }
        "fstatat" | "fstatat64" | "readlinkat" | "ioctl" => &[WritesOutside { arg: 2, depth: 0 }],
        "statx" => &[WritesOutside { arg: 4, depth: 0 }],
        "readv" | "preadv" | "preadv2" => &[WritesOutside { arg: 1, depth: 1 }],
        "recvfrom" => &[
            WritesOutside { arg: 1, depth: 0 },
            WritesOutside { arg: 4, depth: 0 },
            WritesOutside { arg: 5, depth: 0 },
        ],
        "recvmsg" | "recvmmsg" => &[
            WritesOutside { arg: 1, depth: 0 },
            WritesOutside { arg: 1, depth: 1 },
            WritesOutside { arg: 1, depth: 2 },
        ],
        "getsockopt" => &[
            WritesOutside { arg: 3, depth: 0 },
            WritesOutside { arg: 4, depth: 0 },
        ],
        "getsockname" | "getpeername" | "accept" | "accept4" => &[
            WritesOutside { arg: 1, depth: 0 },
            WritesOutside { arg: 2, depth: 0 },
        ],
        "select" | "pselect" => &[
            WritesOutside { arg: 1, depth: 0 },
            WritesOutside { arg: 2, depth: 0 },
            WritesOutside { arg: 3, depth: 0 },
        ],
        "socketpair" => &[WritesOutside { arg: 3, depth: 0 }],
        "epoll_ctl" => &[HoldsHeld(3)],
        "epoll_wait" | "epoll_pwait" => &[WritesHeld(1), WritesOutside { arg: 1, depth: 0 }],
        "fread" | "fread_unlocked" => &[WritesOutside { arg: 0, depth: 0 }, Runs(Later::Stream)],
        "strchr" | "strrchr" | "strchrnul" | "memchr" | "memrchr" | "rawmemchr" | "strstr"
        | "strcasestr" | "strpbrk" | "memmem" | "index" | "rindex" | "strcpy" | "strncpy"
        | "stpcpy" | "stpncpy" | "strcat" | "strncat" | "memset" | "wcschr" | "wcsrchr"
        | "wcsstr" | "wcscpy" | "wcsncpy" | "wmemset" | "wmemchr" | "assume_aligned"
        | "mkdtemp" | "mktemp" | "strptime" => &[Gives(0)],
        "gets" => &[Gives(0), WritesOutside { arg: 0, depth: 0 }],
        "fgets" | "fgets_unlocked" | "fgetws" | "fgetws_unlocked" => &[
            Gives(0),
            WritesOutside { arg: 0, depth: 0 },
            Runs(Later::Stream),
        ],
        "memccpy" => &[
            Copies {
                into: 0,
                from: 1,
                len: Some(3),
            },
            Gives(0),
        ],
        "basename" | "__xpg_basename" | "dirname" | "strtok" | "tmpnam" | "ctermid" => {
            &[Gives(0), GivesOwn]
        }
        "gcvt" => &[Gives(2)],
        "crypt_r" => &[Gives(2)],
        "if_indextoname" => &[Gives(1)],
        "dlopen" | "dlmopen" | "dlsym" | "dlvsym" => &[GivesLoaded],
        "getopt" | "getopt_long" | "getopt_long_only" => &[Holds(1)],
        "putenv" | "openlog" => &[Holds(0)],
        "ns_initparse" => &[WritesInto {
            through: 2,
            from: 0,
        }],
        "ns_parserr" => &[Copies {
            into: 3,
            from: 0,
            len: None,
        }],
        "ctime_r" | "asctime_r" => &[Gives(1)],
        "gmtime_r" | "localtime_r" => &[Gives(1), WritesOwn(1)],
        "inet_ntop" => &[Gives(2)],
        "strsep" => &[GivesHeld(0)],
        "strtok_r" => &[
            Gives(0),
            GivesHeld(2),
            WritesInto {
                through: 2,
                from: 0,
            },
        ],
        "strerror_r" => &[Gives(1), GivesOwn],
        "strtol" | "strtoul" | "strtoll" | "strtoull" | "strtoq" | "strtouq" | "strtod"
        | "strtof" | "strtold" | "strtoimax" | "strtoumax" | "wcstol" | "wcstoul" | "wcstoll"
        | "wcstoull" | "wcstod" | "strtol_l" | "strtod_l" => &[WritesInto {
            through: 1,
            from: 0,
        }],
        "getenv"
        | "secure_getenv"
        | "strerror"
        | "strsignal"
        | "localtime"
        | "gmtime"
        | "ctime"
        | "asctime"
        | "setlocale"
        | "localeconv"
        | "nl_langinfo"
        | "getpwnam"
        | "getpwuid"
        | "getgrnam"
        | "getgrgid"
        | "getlogin"
        | "gethostbyname"
        | "gethostbyname2"
        | "gethostbyaddr"
        | "getservbyname"
        | "getservbyport"
        | "getprotobyname"
        | "getprotobynumber"
        | "inet_ntoa"
        | "ttyname"
        | "readdir"
        | "readdir64"
        | "opendir"
        | "fdopendir"
        | "fopen"
        | "fopen64"
        | "fdopen"
        | "popen"
        | "tmpfile"
        | "tmpfile64"
        | "dlerror"
        | "crypt"
        | "getpass"
        | "if_nameindex"
        | "gai_strerror"
        | "hstrerror"
        | "ptsname"
        | "getmntent"
        | "__errno_location"
        | "__h_errno_location"
        | "__ctype_b_loc"
        | "__ctype_tolower_loc"
        | "__ctype_toupper_loc"
        | "newlocale"
        | "duplocale"
        | "uselocale"
        | "iconv_open"
        | "getspnam"
        | "getpwent"
        | "getgrent"
        | "fmemopen"
        | "open_memstream"
        | "pthread_getspecific"
        | "pthread_self"
        | "strerror_l"
        | "nl_langinfo_l"
        | "fts_read"
        | "fts_children"
        | "seed48"
        | "ecvt"
        | "fcvt"
        | "l64a"
        | "ether_ntoa"
        | "gnu_get_libc_version"
        | "getutent"
        | "sem_open"
        | "getservent"
        | "getprotoent"
        | "getnetbyname"
        | "getnetent"
        | "getmntent_r" => &[GivesOwn],
        "pthread_join" => &[WritesOwn(1)],
        "getpwnam_r" | "getpwuid_r" | "getgrnam_r" | "getgrgid_r" | "getspnam_r"
        | "gethostbyname_r" | "gethostbyname2_r" => &[
            WritesInto {
                through: 4,
                from: 1,
            },
            Fills {
                through: 1,
                from: 2,
            },
            WritesOutside { arg: 1, depth: 0 },
            WritesOutside { arg: 2, depth: 0 },
        ],
        "getaddrinfo" => &[WritesNew(3), WritesOutside { arg: 3, depth: 1 }],
        "pthread_setspecific" => &[Holds(1)],
        "scandir" | "scandir64" => &[
            WritesNew(1),
            Calls {
                callee: 2,
                args: &[&[Own]],
            },
            Calls {
                callee: 3,
                args: &[&[Own], &[Own]],
            },
        ],
        "qsort" => &[Calls {
            callee: 3,
            args: &[&[Into(0, Some(2))], &[Into(0, Some(2))]],
        }],
        "qsort_r" => &[Calls {
            callee: 3,
            args: &[&[Into(0, Some(2))], &[Into(0, Some(2))], &[Arg(4)]],
        }],
        "bsearch" | "lfind" | "lsearch" => &[
            Gives(1),
            Calls {
                callee: 4,
                args: &[&[Arg(0)], &[Into(1, Some(3))]],
            },
        ],
        "tsearch" | "tfind" | "tdelete" => &[
            Holds(0),
            GivesOwn,
            Calls {
                callee: 2,
                args: &[&[Arg(0), Own], &[Arg(0), Own]],
            },
        ],
        "twalk" | "tdestroy" => &[Calls {
            callee: 1,
            args: &[&[Own]],
        }],
        "twalk_r" => &[Calls {
            callee: 1,
            args: &[&[Own], &[], &[Arg(2)]],
        }],
        "ftw" | "nftw" | "ftw64" | "nftw64" => &[Calls {
            callee: 1,
            args: &[&[Arg(0), Own], &[Own], &[], &[Own]],
        }],
        "fts_open" => &[
            GivesOwn,
            Calls {
                callee: 2,
                args: &[&[Own], &[Own]],
            },
        ],
        "glob" | "glob64" => &[Calls {
            callee: 2,
            args: &[&[Arg(0), Own]],
        }],
        "dl_iterate_phdr" => &[Calls {
            callee: 0,
            args: &[&[Own], &[], &[Arg(1)]],
        }],
        "pthread_once" | "call_once" => &[Calls {
            callee: 1,
            args: &[],
        }],
        "pthread_create" => &[Keeps {
            callee: 2,
            args: &[&[Arg(3)]],
            when: Later::Thread,
        }],
        "atexit" | "at_quick_exit" => &[Keeps {
            callee: 0,
            args: &[],
            when: Later::Exit,
        }],
        "on_exit" => &[Keeps {
            callee: 0,
            args: &[&[], &[Arg(1)]],
            when: Later::Exit,
        }],
        "__cxa_atexit" => &[Keeps {
            callee: 0,
            args: &[&[Arg(1)]],
            when: Later::Exit,
        }],
        "signal" | "sysv_signal" | "bsd_signal" | "sigset" => &[
            Keeps {
                callee: 1,
                args: &[],
                when: Later::Signal,
            },
            GivesOwn,
        ],
        "sigaction" => &[
            KeepsMembers {
                arg: 1,
                record: "sigaction",
                when: Later::Signal,
            },
            WritesHeld(2),
        ],
        "timer_create" => &[
            KeepsMembers {
                arg: 1,
                record: "sigevent",
                when: Later::Thread,
            },
            WritesNew(2),
        ],
        "pthread_atfork" => &[
            Keeps {
                callee: 0,
                args: &[],
                when: Later::Fork,
            },
            Keeps {
                callee: 1,
                args: &[],
                when: Later::Fork,
            },
            Keeps {
                callee: 2,
                args: &[],
                when: Later::Fork,
            },
        ],
        "pthread_key_create" => &[Keeps {
            callee: 1,
            args: &[&[Own]],
            when: Later::ThreadExit,
        }],
        "fopencookie" => &[
            KeepsMembers {
                arg: 2,
                record: "_IO_cookie_io_functions_t",
                when: Later::Stream,
            },
            Holds(0),
            GivesOwn,
        ],
        "freopen" | "freopen64" => &[GivesOwn, Runs(Later::Stream)],
        "exit" | "quick_exit" => &[Runs(Later::Exit)],
        "fork" | "forkpty" => &[Runs(Later::Fork)],
        "pthread_exit" => &[Runs(Later::ThreadExit)],
        "raise" | "abort" | "kill" | "killpg" | "pthread_kill" | "sigqueue" | "tgkill" => {
            &[Runs(Later::Signal)]
        }
        name if on_stream(name) => &[Runs(Later::Stream)],
        name if no_pointers(name) => &[],
        _ => return None,
    })
}

/// The C library's functions that read or write a stream they are given,
/// which runs the functions of a stream opened over them
/// (`fopencookie`).
fn on_stream(name: &str) -> bool {
    matches!(
        name,
        "fread"
            | "fread_unlocked"
            | "fwrite"
            | "fwrite_unlocked"
            | "fgetc"
            | "fgetc_unlocked"
            | "getc"
            | "getc_unlocked"
            | "getw"
            | "fputc"
            | "fputc_unlocked"
            | "putc"
            | "putc_unlocked"
            | "putw"
            | "fputs"
            | "fputs_unlocked"
            | "fprintf"
            | "vfprintf"
            | "fscanf"
            | "vfscanf"
            | "fflush"
            | "fflush_unlocked"
            | "fseek"
            | "fseeko"
            | "fseeko64"
            | "ftell"
            | "ftello"
            | "ftello64"
            | "rewind"
            | "fgetpos"
            | "fgetpos64"
            | "fsetpos"
            | "fsetpos64"
            | "fclose"
            | "fcloseall"
            | "ungetc"
            | "__overflow"
            | "__uflow"
            | "__underflow"
            | "fgetwc"
            | "fputwc"
            | "getwc"
            | "putwc"
            | "fputws"
            | "fwprintf"
            | "vfwprintf"
            | "fwscanf"
            | "ungetwc"
            | "fpurge"
            | "__fpurge"
    )
}

/// The C library's functions that neither keep, give nor write a pointer
/// nor call code: they read memory, or write bytes and numbers.
fn no_pointers(name: &str) -> bool {
    if math(name) {
        return true;
    }
    matches!(
        name,
        // Strings and characters.
        "strlen" | "strnlen" | "wcslen" | "wcsnlen" | "strcmp" | "strncmp" | "strcasecmp"
            | "strncasecmp" | "strcasecmp_l" | "strncasecmp_l" | "memcmp" | "bcmp" | "wmemcmp"
            | "wcscmp" | "wcsncmp" | "wcscasecmp" | "strcoll" | "strcoll_l" | "wcscoll"
            | "strxfrm" | "strxfrm_l" | "wcsxfrm" | "strspn" | "strcspn" | "wcsspn"
            | "wcscspn" | "strverscmp" | "strlcpy" | "strlcat" | "strfry" | "memfrob" | "swab"
            | "bzero" | "explicit_bzero" | "memset_explicit" | "atoi" | "atol" | "atoll"
            | "atof" | "a64l" | "strfmon" | "toupper" | "tolower" | "toupper_l" | "tolower_l"
            | "towupper" | "towlower" | "isalnum" | "isalpha" | "isascii" | "isblank"
            | "iscntrl" | "isdigit" | "isgraph" | "islower" | "isprint" | "ispunct"
            | "isspace" | "isupper" | "isxdigit" | "iswalnum" | "iswalpha" | "iswblank"
            | "iswcntrl" | "iswdigit" | "iswgraph" | "iswlower" | "iswprint" | "iswpunct"
            | "iswspace" | "iswupper" | "iswxdigit" | "isalnum_l" | "isalpha_l" | "isdigit_l"
            | "isspace_l" | "isupper_l" | "islower_l" | "isxdigit_l" | "isprint_l"
            | "iswctype" | "wctype" | "wctrans" | "towctrans" | "mbrtowc" | "mbrlen"
            | "__mbrlen" | "mbstowcs" | "wcstombs" | "mbtowc" | "wctomb" | "btowc"
            | "__btowc_alias" | "wctob" | "__wctob_alias" | "mbsinit" | "mbrtoc16" | "mbrtoc32"
            | "c16rtomb" | "c32rtomb" | "wcrtomb" | "wcwidth" | "wcswidth" | "mblen"
            | "mbsrtowcs" | "wcsrtombs" | "mbsnrtowcs" | "wcsnrtombs" | "__ctype_get_mb_cur_max"
            | "iconv" | "iconv_close" | "regcomp" | "regexec" | "regfree" | "regerror"
            | "fnmatch" | "sprintf" | "snprintf" | "vsprintf" | "vsnprintf" | "dprintf"
            | "vdprintf" | "sscanf" | "vsscanf" | "printf" | "vprintf" | "puts" | "putchar"
            | "putchar_unlocked" | "getchar" | "getchar_unlocked" | "scanf" | "vscanf"
            | "perror" | "psignal" | "__xpg_strerror_r"
            // Numbers.
            | "abs" | "labs" | "llabs" | "imaxabs" | "div" | "ldiv" | "lldiv" | "imaxdiv"
            | "rand" | "rand_r" | "srand" | "random" | "srandom" | "random_r" | "srandom_r"
            | "drand48" | "erand48" | "lrand48" | "nrand48" | "mrand48" | "jrand48" | "srand48"
            | "lcong48" | "getrandom" | "getentropy" | "arc4random" | "arc4random_buf"
            | "arc4random_uniform"
            // Memory.
            | "free" | "cfree" | "malloc_usable_size" | "malloc_trim" | "mallopt"
            | "mallinfo" | "mallinfo2" | "malloc_stats" | "munmap" | "mprotect" | "madvise"
            | "posix_madvise" | "mlock" | "munlock" | "mlockall" | "munlockall" | "msync"
            | "mincore" | "memfd_create" | "shmget" | "shmctl" | "shmdt" | "semget" | "semop"
            | "semctl" | "msgget" | "msgsnd" | "msgrcv" | "msgctl" | "ftok" | "shm_open"
            | "shm_unlink" | "__sched_cpufree" | "__sched_cpucount"
            // Time.
            | "time" | "clock" | "clock_gettime" | "clock_getres" | "clock_settime"
            | "clock_nanosleep" | "gettimeofday" | "settimeofday" | "mktime" | "timegm"
            | "timelocal" | "difftime" | "tzset" | "nanosleep" | "sleep" | "usleep" | "alarm"
            | "setitimer" | "getitimer" | "times" | "getrusage" | "strftime" | "strftime_l"
            | "wcsftime" | "ftime" | "timer_settime" | "timer_gettime" | "timer_delete"
            // Processes and signals.
            | "getpid" | "getppid" | "gettid" | "getuid" | "geteuid" | "getgid" | "getegid"
            | "setuid" | "setgid" | "seteuid" | "setegid" | "setreuid" | "setregid"
            | "setresuid" | "setresgid" | "getresuid" | "getresgid" | "setsid" | "getsid"
            | "setpgid" | "getpgid" | "getpgrp" | "getpriority" | "setpriority" | "nice"
            | "getrlimit" | "setrlimit" | "getrlimit64" | "setrlimit64" | "prlimit"
            | "getgroups" | "setgroups" | "initgroups" | "getgrouplist" | "umask" | "chroot"
            | "_exit" | "_Exit" | "execv" | "execve" | "execvp" | "execvpe" | "execl"
            | "execlp" | "execle" | "fexecve" | "waitpid" | "wait" | "wait3" | "wait4"
            | "waitid" | "system" | "vfork" | "posix_spawn" | "posix_spawnp"
            | "posix_spawn_file_actions_init" | "posix_spawn_file_actions_destroy"
            | "posix_spawn_file_actions_addopen" | "posix_spawn_file_actions_adddup2"
            | "posix_spawn_file_actions_addclose" | "posix_spawn_file_actions_addchdir_np"
            | "posix_spawnattr_init" | "posix_spawnattr_destroy" | "posix_spawnattr_setflags"
            | "posix_spawnattr_setsigmask" | "posix_spawnattr_setsigdefault"
            | "posix_spawnattr_setpgroup" | "sigemptyset" | "sigfillset" | "sigaddset"
            | "sigdelset" | "sigismember" | "sigprocmask" | "pthread_sigmask" | "sigsuspend"
            | "pause" | "sigwait" | "sigwaitinfo" | "sigtimedwait" | "sigpending"
            | "sigaltstack" | "siginterrupt" | "setjmp" | "_setjmp" | "sigsetjmp"
            | "__sigsetjmp" | "longjmp" | "_longjmp" | "siglongjmp" | "prctl" | "ptrace"
            | "personality" | "syscall" | "__libc_current_sigrtmin" | "__libc_current_sigrtmax"
            | "backtrace" | "backtrace_symbols_fd"
            // Files and descriptors.
            | "open" | "open64" | "openat" | "openat64" | "creat" | "creat64" | "close"
            | "read" | "write" | "pread" | "pread64" | "pwrite" | "pwrite64" | "readv"
            | "writev" | "preadv" | "pwritev" | "lseek" | "lseek64" | "dup" | "dup2" | "dup3"
            | "pipe" | "pipe2" | "fcntl" | "fcntl64" | "ioctl" | "fsync" | "fdatasync"
            | "ftruncate" | "ftruncate64" | "truncate" | "truncate64" | "flock" | "lockf"
            | "posix_fallocate" | "fallocate" | "posix_fadvise" | "poll" | "ppoll" | "select"
            | "pselect" | "epoll_create" | "epoll_create1" | "epoll_ctl" | "epoll_wait"
            | "epoll_pwait" | "eventfd" | "signalfd" | "timerfd_create" | "timerfd_settime"
            | "inotify_init" | "inotify_init1" | "inotify_add_watch" | "inotify_rm_watch"
            | "sendfile" | "copy_file_range" | "splice" | "stat" | "stat64" | "fstat"
            | "fstat64" | "lstat" | "lstat64" | "fstatat" | "fstatat64" | "statx" | "statvfs"
            | "statvfs64" | "fstatvfs" | "fstatvfs64" | "statfs" | "fstatfs" | "access"
            | "faccessat" | "euidaccess" | "eaccess" | "unlink" | "unlinkat" | "rename"
            | "renameat" | "renameat2" | "mkdir" | "mkdirat" | "rmdir" | "chdir" | "fchdir"
            | "chmod" | "fchmod" | "fchmodat" | "chown" | "fchown" | "lchown" | "fchownat"
            | "utime" | "utimes" | "futimes" | "lutimes" | "futimens" | "utimensat" | "link"
            | "linkat" | "symlink" | "symlinkat" | "readlink" | "readlinkat" | "mkfifo"
            | "mknod" | "mkstemp" | "mkstemp64" | "mkstemps" | "mkostemp" | "closedir"
            | "rewinddir" | "seekdir" | "telldir" | "dirfd" | "globfree" | "globfree64"
            | "fts_close" | "alphasort" | "alphasort64" | "versionsort" | "feof" | "ferror"
            | "clearerr" | "fileno" | "feof_unlocked" | "ferror_unlocked" | "fileno_unlocked"
            | "clearerr_unlocked" | "setvbuf" | "setbuf" | "setbuffer" | "setlinebuf"
            | "__fpending" | "__freadable" | "__fwritable" | "__flbf" | "__fbufsize"
            | "__fsetlocking" | "remove" | "isatty" | "tcgetattr" | "tcsetattr" | "tcflush"
            | "tcdrain" | "cfmakeraw" | "cfgetispeed" | "cfsetispeed" | "cfgetospeed"
            | "cfsetospeed" | "tcgetpgrp" | "tcsetpgrp" | "openpty" | "login_tty" | "grantpt"
            | "unlockpt" | "ptsname_r" | "posix_openpt" | "ttyname_r" | "getlogin_r"
            | "wordexp" | "wordfree" | "freeifaddrs" | "pathconf" | "fpathconf"
            // Sockets and names.
            | "socket" | "bind" | "connect" | "listen" | "accept" | "accept4" | "send"
            | "sendto" | "recv" | "recvfrom" | "sendmsg" | "recvmsg" | "shutdown"
            | "getsockopt" | "setsockopt" | "getsockname" | "getpeername" | "socketpair"
            | "inet_pton" | "inet_addr" | "inet_aton" | "inet_network" | "htons" | "htonl"
            | "ntohs" | "ntohl" | "freeaddrinfo" | "gethostname" | "sethostname"
            | "getdomainname" | "getnameinfo" | "if_nametoindex" | "if_freenameindex"
            | "res_init" | "res_ninit" | "res_nclose" | "res_search" | "res_query"
            | "res_nsearch" | "res_nquery" | "res_querydomain" | "dn_expand" | "dn_skipname"
            | "dn_comp" | "__dn_expand" | "__dn_skipname" | "__res_search" | "__res_nsearch"
            | "__res_ninit" | "__res_nclose" | "__res_init" | "__res_query" | "__res_nquery"
            | "ns_name_uncompress" | "ns_get16" | "ns_get32" | "endpwent" | "setpwent"
            | "endgrent" | "setgrent" | "endspent" | "setspent" | "setutent" | "endutent"
            | "endservent" | "endprotoent" | "endhostent" | "sethostent"
            // The system.
            | "getpagesize" | "sysconf" | "getdtablesize" | "get_nprocs" | "get_nprocs_conf"
            | "uname" | "confstr" | "gethostid" | "getloadavg" | "getauxval" | "syslog"
            | "vsyslog" | "closelog" | "setlogmask" | "setenv" | "unsetenv" | "clearenv"
            | "freelocale" | "dlclose" | "dladdr" | "dladdr1"
            // Threads.
            | "pthread_mutex_init" | "pthread_mutex_destroy" | "pthread_mutex_lock"
            | "pthread_mutex_trylock" | "pthread_mutex_unlock" | "pthread_mutex_timedlock"
            | "pthread_mutexattr_init" | "pthread_mutexattr_destroy"
            | "pthread_mutexattr_settype" | "pthread_mutexattr_setpshared"
            | "pthread_cond_init" | "pthread_cond_destroy" | "pthread_cond_wait"
            | "pthread_cond_timedwait" | "pthread_cond_signal" | "pthread_cond_broadcast"
            | "pthread_condattr_init" | "pthread_condattr_destroy"
            | "pthread_condattr_setpshared" | "pthread_condattr_setclock"
            | "pthread_rwlock_init" | "pthread_rwlock_destroy" | "pthread_rwlock_rdlock"
            | "pthread_rwlock_wrlock" | "pthread_rwlock_tryrdlock" | "pthread_rwlock_trywrlock"
            | "pthread_rwlock_unlock" | "pthread_rwlockattr_init"
            | "pthread_rwlockattr_destroy" | "pthread_rwlockattr_setpshared"
            | "pthread_spin_init" | "pthread_spin_destroy" | "pthread_spin_lock"
            | "pthread_spin_unlock" | "pthread_attr_init" | "pthread_attr_destroy"
            | "pthread_attr_setstacksize" | "pthread_attr_getstacksize"
            | "pthread_attr_setdetachstate" | "pthread_attr_getguardsize"
            | "pthread_attr_getstack" | "pthread_getattr_np" | "pthread_detach"
            | "pthread_cancel" | "pthread_setcancelstate" | "pthread_setcanceltype"
            | "pthread_testcancel" | "pthread_equal" | "pthread_key_delete"
            | "pthread_setname_np" | "pthread_getname_np" | "sched_yield"
            | "sched_getaffinity" | "sched_setaffinity" | "sched_getcpu" | "sem_init"
            | "sem_destroy" | "sem_wait" | "sem_trywait" | "sem_timedwait" | "sem_post"
            | "sem_getvalue" | "sem_close" | "sem_unlink"
            // Failures, which do not return.
            | "__stack_chk_fail" | "__assert_fail" | "__assert_perror_fail" | "__fortify_fail"
            | "__chk_fail" | "__fdelt_chk" | "__fdelt_warn"
    )
}

/// The C library's mathematical functions, which compute on numbers: each
/// in its `double`, `float` (`f`) and `long double` (`l`) forms.
fn math(name: &str) -> bool {
    const FUNCTIONS: &[&str] = &[
        "sin",
        "cos",
        "tan",
        "asin",
        "acos",
        "atan",
        "atan2",
        "sinh",
        "cosh",
        "tanh",
        "asinh",
        "acosh",
        "atanh",
        "exp",
        "exp2",
        "exp10",
        "expm1",
        "log",
        "log10",
        "log2",
        "log1p",
        "logb",
        "ilogb",
        "pow",
        "sqrt",
        "cbrt",
        "hypot",
        "ceil",
        "floor",
        "round",
        "trunc",
        "rint",
        "nearbyint",
        "lround",
        "llround",
        "lrint",
        "llrint",
        "fmod",
        "remainder",
        "remquo",
        "fabs",
        "copysign",
        "nextafter",
        "nexttoward",
        "frexp",
        "ldexp",
        "scalbn",
        "scalbln",
        "modf",
        "fmin",
        "fmax",
        "fdim",
        "fma",
        "erf",
        "erfc",
        "tgamma",
        "lgamma",
        "lgamma_r",
        "isnan",
        "isinf",
        "isfinite",
        "isnormal",
        "finite",
        "__isnan",
        "__isinf",
        "__finite",
        "__fpclassify",
        "__signbit",
        "__issignaling",
        "sincos",
        "j0",
        "j1",
        "jn",
        "y0",
        "y1",
        "yn",
        "significand",
        "drem",
        "gamma",
        "nan",
    ];
    let bare = name
        .strip_suffix('f')
        .or_else(|| name.strip_suffix('l'))
        .filter(|bare| FUNCTIONS.contains(bare));
    FUNCTIONS.contains(&name) || bare.is_some()
}

/// The compiler's builtins that are no function of the C library's: the
/// ones computing on values, the vector operations of the target's
/// instructions among them, and the ones writing their result through a
/// pointer.
fn builtin(name: &str) -> Option<&'static [Effect]> {
    use Effect::*;
    let rest = name.strip_prefix("__builtin_")?;
    if let Some(instruction) = rest.strip_prefix("ia32_") {
        let moves_memory = [
            "load", "store", "gather", "scatter", "maskmov", "movnt", "lddqu",
        ]
        .iter()
        .any(|word| instruction.contains(word));
        return match instruction {
            // Bytes and 32-bit values, which hold no pointer.
            "maskmovdqu" | "movnti" => Some(&[]),
            "movnti64" => Some(&[WritesInto {
                through: 0,
                from: 1,
            }]),
            _ if moves_memory => None,
            _ => Some(&[Derives]),
        };
    }
    if rest.starts_with("elementwise_") || rest.starts_with("reduce_") {
        return Some(&[Derives]);
    }
    if rest.ends_with("_overflow") {
        return Some(&[
            WritesInto {
                through: 2,
                from: 0,
            },
            WritesInto {
                through: 2,
                from: 1,
            },
        ]);
    }
    let bits = [
        "bswap",
        "bitreverse",
        "rotateleft",
        "rotateright",
        "clz",
        "ctz",
        "popcount",
        "parity",
        "ffs",
        "clrsb",
    ];
    if bits.iter().any(|op| rest.starts_with(op)) {
        return Some(&[Derives]);
    }
    Some(match rest {
        "nontemporal_store" => &[WritesInto {
            through: 1,
            from: 0,
        }],
        "nontemporal_load" => &[GivesHeld(0)],
        "memcpy_inline" => &[Copies {
            into: 0,
            from: 1,
            len: Some(2),
        }],
        "memset_inline" => &[],
        "frame_address"
        | "return_address"
        | "extract_return_addr"
        | "object_size"
        | "dynamic_object_size"
        | "prefetch"
        | "cpu_init"
        | "cpu_supports"
        | "cpu_is"
        | "readcyclecounter"
        | "readsteadycounter"
        | "isnan"
        | "isinf"
        | "isinf_sign"
        | "isfinite"
        | "isnormal"
        | "issubnormal"
        | "iszero"
        | "issignaling"
        | "isfpclass"
        | "fpclassify"
        | "signbit"
        | "signbitf"
        | "signbitl"
        | "isgreater"
        | "isgreaterequal"
        | "isless"
        | "islessequal"
        | "islessgreater"
        | "isunordered"
        | "huge_val"
        | "huge_valf"
        | "huge_vall"
        | "inf"
        | "inff"
        | "infl"
        | "nan"
        | "nanf"
        | "nanl"
        | "nans"
        | "nansf"
        | "flt_rounds"
        | "is_constant_evaluated" => &[],
        "speculation_safe_value" => &[Derives],
        _ => return None,
    })
}

/// A function's name as the tables name it: a builtin by the function it
/// builds in, a checked variant (`__memcpy_chk`) by the function checked,
/// and the C library's redirections for a standard (`__isoc23_strtol`) by
/// the function redirected.
fn base_name(name: &str) -> &str {
    if name.starts_with("__atomic_")
        || name.starts_with("__c11_atomic_")
        || name.starts_with("__sync_")
    {
        return name;
    }
    let name = name.strip_prefix("__builtin_").unwrap_or(name);
    let name = name
        .strip_prefix("__isoc99_")
        .or_else(|| name.strip_prefix("__isoc23_"))
        .unwrap_or(name);
    match name.strip_suffix("_chk") {
        Some(checked) => checked.trim_start_matches('_'),
        None => name,
    }
}

/// php-src's allocators: each call gives memory of its own, the ones
/// growing a block copy what it held, and a block given back reaches
/// nothing past the call. The arenas' are static, named with their
/// header.
pub(crate) fn php_allocator(name: &str) -> Option<&'static [Effect]> {
    use Effect::*;
    let allocates = matches!(
        name,
        "_emalloc"
            | "_safe_emalloc"
            | "_ecalloc"
            | "_estrdup"
            | "_estrndup"
            | "_emalloc_large"
            | "_emalloc_huge"
            | "__zend_malloc"
            | "__zend_calloc"
            | "__zend_strdup"
            | "_safe_malloc"
            | "zend_strndup"
            | "_zend_mm_alloc"
            | "Zend/zend_arena.h:zend_arena_alloc"
            | "Zend/zend_arena.h:zend_arena_calloc"
    ) || name
        .strip_prefix("_emalloc_")
        .is_some_and(|size| size.bytes().all(|b| b.is_ascii_digit()));
    let frees = matches!(
        name,
        "_efree"
            | "_efree_large"
            | "_efree_huge"
            | "__zend_free"
            | "_zend_mem_block_size"
            | "is_zend_ptr"
    ) || name
        .strip_prefix("_efree_")
        .is_some_and(|size| size.bytes().all(|b| b.is_ascii_digit()));
    match name {
        _ if allocates => Some(&[Allocates]),
        _ if frees => Some(&[Block(0)]),
        "_erealloc" | "_erealloc2" | "_safe_erealloc" | "__zend_realloc" | "_safe_realloc" => {
            Some(&[Grows(0)])
        }
        "_zend_mm_realloc" | "_zend_mm_realloc2" => Some(&[Grows(1)]),
        "_zend_mm_free" | "_zend_mm_block_size" => Some(&[Block(1)]),
        _ => None,
    }
}
