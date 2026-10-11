<?php
// The parameters the stubs document as resources but declare without a type.
// PHP's reflection shows no type for them, so the recorder reads the stubs'
// docblocks: `@param resource $name` on a function whose `$name` has no type.

/** The directory of the phpstorm stubs the recorder reads, which a worker's sandbox binds read-only. */
function phpstorm_stubs_dir(): string
{
    return dirname(__DIR__, 3) . '/vendor/phpstorm-stubs';
}

/**
 * The lowercased names of the functions and methods whose untyped parameters
 * the stubs document as `resource`, each with the parameters' names.
 *
 * @return array<string, array<string, true>>
 */
function untyped_resource_params(string $root): array
{
    $found = [];
    $files = [];
    foreach (new RecursiveIteratorIterator(new RecursiveDirectoryIterator($root, FilesystemIterator::SKIP_DOTS)) as $file) {
        if ($file->getExtension() === 'php') {
            $files[] = $file->getPathname();
        }
    }
    sort($files);
    foreach ($files as $path) {
        $tokens = array_values(array_filter(token_get_all((string) file_get_contents($path)), static fn ($t): bool => !is_array($t) || !in_array($t[0], [T_WHITESPACE, T_COMMENT], true)));
        $namespace = '';
        $class = null;
        $classDepth = null;
        $depth = 0;
        $doc = null;
        $count = count($tokens);
        for ($i = 0; $i < $count; $i++) {
            $token = $tokens[$i];
            if (is_string($token)) {
                if ($token === '{') {
                    $depth++;
                } elseif ($token === '}') {
                    if ($classDepth === $depth) {
                        [$class, $classDepth] = [null, null];
                    }
                    $depth--;
                }
                continue;
            }
            [$id, $text] = $token;
            if ($id === T_DOC_COMMENT) {
                $doc = $text;
                continue;
            }
            if ($id === T_NAMESPACE) {
                $namespace = '';
                for ($i++; $i < $count && $tokens[$i] !== ';' && $tokens[$i] !== '{'; $i++) {
                    $namespace .= is_array($tokens[$i]) ? $tokens[$i][1] : '';
                }
                $namespace = trim($namespace, '\\');
                $i--;
                continue;
            }
            $previous = $tokens[$i - 1] ?? null;
            $afterScope = is_array($previous) && $previous[0] === T_DOUBLE_COLON;
            if (in_array($id, [T_CLASS, T_INTERFACE, T_TRAIT, T_ENUM], true) && !$afterScope) {
                if (isset($tokens[$i + 1]) && is_array($tokens[$i + 1]) && $tokens[$i + 1][0] === T_STRING) {
                    $class = ($namespace === '' ? '' : $namespace . '\\') . $tokens[$i + 1][1];
                    $classDepth = $depth + 1;
                }
                continue;
            }
            if ($id !== T_FUNCTION) {
                continue;
            }
            $name = null;
            $j = $i + 1;
            while (isset($tokens[$j]) && is_string($tokens[$j]) && $tokens[$j] === '&') {
                $j++;
            }
            if (isset($tokens[$j]) && is_array($tokens[$j]) && $tokens[$j][0] === T_STRING) {
                $name = $tokens[$j][1];
            }
            $documented = $doc === null ? [] : resource_doc_names($doc);
            $doc = null;
            if ($name === null || $documented === []) {
                continue;
            }
            // The parameter list, from its opening parenthesis to the matching one.
            while (isset($tokens[$j]) && $tokens[$j] !== '(') {
                $j++;
            }
            $level = 0;
            for (; isset($tokens[$j]); $j++) {
                $t = $tokens[$j];
                if ($t === '(') {
                    $level++;
                    continue;
                }
                if ($t === ')') {
                    if (--$level === 0) {
                        break;
                    }
                    continue;
                }
                if (!is_array($t) || $t[0] !== T_VARIABLE || $level !== 1) {
                    continue;
                }
                $param = substr($t[1], 1);
                if (!isset($documented[$param])) {
                    continue;
                }
                // Untyped when the token before it, past `&` and `...`, is the list's `(` or a `,`.
                $k = $j - 1;
                while (isset($tokens[$k]) && (is_string($tokens[$k]) && $tokens[$k] === '&' || is_array($tokens[$k]) && $tokens[$k][0] === T_ELLIPSIS)) {
                    $k--;
                }
                $before = $tokens[$k] ?? null;
                if ($before === '(' || $before === ',') {
                    $key = strtolower(($class === null ? ($namespace === '' ? '' : $namespace . '\\') : $class . '::') . $name);
                    $found[$key][$param] = true;
                }
            }
            $i = $j;
        }
    }
    return $found;
}

/**
 * The names of the parameters a docblock documents as a `resource`.
 *
 * @return array<string, true>
 */
function resource_doc_names(string $doc): array
{
    $names = [];
    if (preg_match_all('/@param\s+resource(?:\|null)?\s+\$(\w+)/', $doc, $matches) > 0) {
        foreach ($matches[1] as $name) {
            $names[$name] = true;
        }
    }
    return $names;
}
