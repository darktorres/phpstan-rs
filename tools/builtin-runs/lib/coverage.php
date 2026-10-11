<?php
// What a run did not record, and why: the builtins of this PHP with no
// `probed` row, and the declarations of the engine's stub index that have none.

/**
 * One line for each builtin of this PHP with no `probed` row, and each
 * declaration of the stub index (`crates/stub-sources/index.tsv`) that has no
 * row, by extension: `section`, `extension`, `name`, `reason`.
 *
 * @param array<string, true> $probed the builtins with a `probed` row
 * @param array<string, true> $failed the builtins whose unit was lost twice
 * @param Closure(string): bool $selected the selection of the run
 * @param list<string> $asserted the functions whose `runs nothing` row is asserted, not measured
 * @return list<string>
 */
function coverage_lines(array $probed, array $failed, Closure $selected, array $asserted): array
{
    $lines = [];
    foreach ($asserted as $function) {
        $reflection = new ReflectionFunction($function);
        $lines[] = "builtin\t" . ($reflection->getExtensionName() ?: 'none') . "\t$function\tasserted, not measured";
    }
    foreach (get_defined_functions()['internal'] as $function) {
        $builtin = strtolower($function);
        if (isset($probed[$builtin])) {
            continue;
        }
        $reflection = new ReflectionFunction($function);
        $lines[] = coverage_line('builtin', (string) ($reflection->getExtensionName() ?: 'none'), $function, $builtin, $selected, $failed, in_array($function, SKIPPED, true) || str_starts_with($function, 'posix_') || str_starts_with($function, 'pcntl_'), 'no call reached it');
    }
    foreach (probed_classes() as $class) {
        $reflection = new ReflectionClass($class);
        $abstract = $reflection->isAbstract() || $reflection->isInterface();
        foreach (own_methods($reflection) as $method) {
            $builtin = strtolower($class . '::' . $method->getName());
            if (isset($probed[$builtin])) {
                continue;
            }
            $lines[] = coverage_line('builtin', (string) ($reflection->getExtensionName() ?: 'none'), $class . '::' . $method->getName(), $builtin, $selected, $failed, false, $abstract ? 'abstract or interface' : 'no call reached it');
        }
    }
    $classesWithRows = [];
    foreach (array_keys($probed) as $builtin) {
        if (str_contains($builtin, '::')) {
            $classesWithRows[explode('::', $builtin, 2)[0]] = true;
        }
    }
    foreach (stub_declarations() as [$kind, $key, $name, $extension]) {
        if ($kind === 'function' ? isset($probed[$key]) : isset($classesWithRows[$key])) {
            continue;
        }
        $reason = extension_loaded($extension) ? 'loaded, no row' : 'extension not loaded';
        $lines[] = "stub\t$extension\t$name\t$reason";
    }
    sort($lines, SORT_STRING);
    return $lines;
}

/**
 * The line for one builtin with no row: why it has none.
 *
 * @param array<string, true> $failed
 */
function coverage_line(string $section, string $extension, string $name, string $builtin, Closure $selected, array $failed, bool $skipped, string $otherwise): string
{
    $reason = match (true) {
        !$selected($builtin) => 'not selected',
        $skipped => 'skipped by the recorder',
        isset($failed[$builtin]) => 'lost twice',
        default => $otherwise,
    };
    return "$section\t$extension\t$name\t$reason";
}

/**
 * The functions and classes the engine's stub index declares, each as
 * `[kind, lowercased key, name, extension]`.
 *
 * @return list<array{string, string, string, string}>
 */
function stub_declarations(): array
{
    $declarations = [];
    $lines = @file(dirname(__DIR__, 3) . '/crates/stub-sources/index.tsv', FILE_IGNORE_NEW_LINES) ?: [];
    foreach (array_slice($lines, 1) as $line) {
        $fields = explode("\t", $line);
        if (count($fields) < 6 || $fields[5] === '' || !in_array($fields[0], ['function', 'class'], true)) {
            continue;
        }
        $declarations[] = [$fields[0], strtolower($fields[1]), $fields[2], $fields[5]];
    }
    return $declarations;
}
