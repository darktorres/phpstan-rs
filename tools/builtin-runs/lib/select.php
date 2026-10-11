<?php
// Which builtins a run records: the ones a regular expression matches on their
// lowercased names, the ones of some extensions, or both.

/**
 * The lowercased names of the functions and methods of the extensions.
 *
 * @param list<string> $extensions
 * @return array<string, true>
 */
function extension_builtins(array $extensions): array
{
    $names = [];
    foreach ($extensions as $extension) {
        if (!extension_loaded($extension)) {
            fwrite(STDERR, "extension $extension is not loaded\n");
            exit(2);
        }
        $reflection = new ReflectionExtension($extension);
        foreach (array_keys($reflection->getFunctions()) as $name) {
            $names[strtolower($name)] = true;
        }
        foreach ($reflection->getClasses() as $class) {
            foreach ($class->getMethods() as $method) {
                if ($method->getDeclaringClass()->getName() === $class->getName()) {
                    $names[strtolower($class->getName() . '::' . $method->getName())] = true;
                }
            }
        }
    }
    return $names;
}

/**
 * The selection of a run: a builtin is selected when the regular expression
 * matches its lowercased name (when given) and it belongs to the extensions
 * (when given).
 *
 * @param list<string>|null $extensions
 * @return Closure(string): bool
 */
function selector(?string $only, ?array $extensions): Closure
{
    $names = $extensions === null ? null : extension_builtins($extensions);
    return static fn (string $builtin): bool => ($only === null || preg_match($only, $builtin) === 1)
        && ($names === null || isset($names[$builtin]));
}
