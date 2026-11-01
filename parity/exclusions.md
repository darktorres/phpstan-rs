# Exclusions from the parity exit

The parity exit is 100% of every suite, minus this file. Each entry is added with its reason, in the commit that adds it. Every reason is one of these kinds:
- **won't-port:** for example PHPStan's own `Api` or `Playground` rules, re-checked against the source;
- **needs PHP execution:** the test needs PHP to run, as PHPStan does in-process (DI `services:`, executing `bootstrapFiles`, `objectManagerLoader`, `consoleApplicationLoader`);
- **display form:** the normalization can't cover it.

| suite | test | kind | reason |
|---|---|---|---|

Pending from the legacy (CA-4): the 14 rule-test files that need php-parser or PHPStan classes loaded. They are listed by name once the rules suite's extractor runs.
