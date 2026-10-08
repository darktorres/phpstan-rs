# Builtin runs

`record.php` calls every function and every public method of every class
the running PHP defines, with values at each parameter that record what
user code PHP runs through them: a callable (invoked, or kept without being
invoked), an object (which of its methods PHP calls), a URL or a stream of
a recording stream wrapper (which wrapper methods PHP calls), and, for a
method, an object made with such a value at a constructor parameter, or
given it by another method of the class called first (what of it the
method runs). Each call runs in a child process of its own, in an
empty directory, with an alarm.

The effects of calls (`crates/phpstan-rs/src/registry/effects.rs`) read
the table it prints: a builtin writes what the code it runs writes. A
builtin missing from the table, one this PHP does not define or could not
call (a class whose objects only PHP makes, such as `Generator`), may run
any code. Regenerate it for a new PHP version, or to cover more
extensions:

    php tools/builtin-runs/record.php --jobs=6 > crates/phpstan-rs/src/registry/data/builtin-runs-8.5.tsv

`--jobs` sets the number of workers, one for each processor by default.
