<?php declare(strict_types = 1);

// Materializes the parser and lexer suites' inputs that are not plain files
// (docs/design/scoreboard.md, "parser"): the code sections of nikic's parser
// fixtures, the FILE sections of php-src's .phpt tests, and the generated
// operator pairs.
//
// Usage: php inputs.php <phpstan-src> <nikic-php-parser> <php-src> <store> > manifest.tsv
//
// Each input is written to <store>/<sha256 of its bytes>.php, so equal
// inputs share a file and a store is never stale. One manifest line per
// input: its case id, a tab, and its file. Case ids are
// nikic/<fixture path>#<n> (the fixture's n-th code section, from 0) and
// php-src/<test path> and operators/<expression>.

[, , $nikic, $phpSrc, $store] = $argv + [null, null, null, null, null];
if ($nikic === null || $phpSrc === null || $store === null) {
	throw new RuntimeException('usage: inputs.php <phpstan-src> <nikic-php-parser> <php-src> <store>');
}
if (!is_dir($store) && !mkdir($store, 0777, true)) {
	throw new RuntimeException("cannot create $store");
}

/** @return list<string> paths under $dir with $suffix, relative, sorted */
function files(string $dir, string $suffix): array
{
	$out = [];
	$iterator = new RecursiveIteratorIterator(new RecursiveDirectoryIterator($dir, FilesystemIterator::SKIP_DOTS));
	foreach ($iterator as $file) {
		if (str_ends_with($file->getFilename(), $suffix)) {
			$out[] = str_replace('\\', '/', substr($file->getPathname(), strlen($dir) + 1));
		}
	}
	sort($out, SORT_STRING);
	return $out;
}

function emit(string $store, string $id, string $code): void
{
	$path = $store . '/' . hash('sha256', $code) . '.php';
	if (!is_file($path)) {
		$temporary = $path . '.tmp';
		file_put_contents($temporary, $code);
		rename($temporary, $path);
	}
	echo $id, "\t", $path, "\n";
}

// nikic's fixtures, read as its CodeTestParser reads them: canonicalized,
// @@{expr}@@ evaluated, split on "-----" lines into a name and then pairs of
// code and expected output. The code is what PhpParser's tests parse.
$skippedNikic = 0;
foreach (files($nikic . '/test/code/parser', '.test') as $relative) {
	$text = (string) file_get_contents($nikic . '/test/code/parser/' . $relative);
	$text = str_replace("\r\n", "\n", $text);
	$text = rtrim($text, "\n");
	$text = implode("\n", array_map(static fn (string $line): string => rtrim($line, " \t"), explode("\n", $text)));
	$text = preg_replace_callback('/@@\{(.*?)\}@@/', static fn (array $m): string => (string) eval('return ' . $m[1] . ';'), $text);
	$parts = preg_split("/\n-----(?:\n|$)/", (string) $text) ?: [];
	array_shift($parts);
	foreach (array_chunk($parts, 2) as $n => $pair) {
		if (count($pair) !== 2) {
			$skippedNikic++;
			continue;
		}
		emit($store, 'nikic/test/code/parser/' . $relative . '#' . $n, $pair[0]);
	}
}

// php-src's tests, read as run-tests.php reads them: a section starts at a
// "--NAME--" line; a FILE section ends early at "===DONE==="; FILEEOF drops
// its trailing line breaks; FILE_EXTERNAL names a file beside the test.
// REDIRECTTEST tests and tests without a FILE section have no input.
$skippedPhpt = 0;
foreach (files($phpSrc, '.phpt') as $relative) {
	$sections = [];
	$section = null;
	$done = false;
	$handle = fopen($phpSrc . '/' . $relative, 'rb') ?: throw new RuntimeException("cannot read $relative");
	while (($line = fgets($handle)) !== false) {
		if (preg_match('/^--([_A-Z]+)--/', $line, $match) === 1) {
			$section = $match[1];
			$sections[$section] = '';
			$done = false;
			continue;
		}
		if ($section === null || $done) {
			continue;
		}
		$sections[$section] .= $line;
		if (in_array($section, ['FILE', 'FILEEOF', 'FILE_EXTERNAL'], true) && preg_match('/^===DONE===\s*$/', $line) === 1) {
			$done = true;
		}
	}
	fclose($handle);
	if (isset($sections['REDIRECTTEST'])) {
		$skippedPhpt++;
		continue;
	}
	if (isset($sections['FILEEOF'])) {
		$sections['FILE'] = (string) preg_replace("/[\r\n]+$/", '', $sections['FILEEOF']);
	}
	if (isset($sections['FILE_EXTERNAL'])) {
		$external = dirname($phpSrc . '/' . $relative) . '/' . trim(str_replace('..', '', $sections['FILE_EXTERNAL']));
		if (!is_file($external)) {
			$skippedPhpt++;
			continue;
		}
		$sections['FILE'] = (string) file_get_contents($external);
	}
	if (!isset($sections['FILE'])) {
		$skippedPhpt++;
		continue;
	}
	emit($store, 'php-src/' . $relative, $sections['FILE']);
}

// Every ordered pair of operator forms, the inner form in the outer one's
// first and last operand, bare and parenthesized: the bare one shows how
// precedence and associativity group them, or that PHP refuses the pair.
// "#" marks an operand; operands are named $a, $b, ... left to right. Each
// expression is returned from a function, so yield is allowed.
$forms = [];
foreach (['or', 'xor', 'and', '??', '||', '&&', '|', '^', '&', '==', '!=', '===', '!==', '<>', '<=>', '<', '<=', '>', '>=', '<<', '>>', '+', '-', '*', '/', '%', '.', '**', 'instanceof', '|>', '?:'] as $operator) {
	$forms[] = "# $operator #";
}
foreach (['=', '+=', '-=', '*=', '/=', '.=', '%=', '**=', '&=', '|=', '^=', '<<=', '>>=', '??=', '=&'] as $operator) {
	$forms[] = "# $operator #";
}
$forms[] = '# ? # : #';
foreach (['!', '-', '+', '~', '@', '++', '--', '(int) ', '(float) ', '(string) ', '(bool) ', '(array) ', '(object) ', 'clone ', 'print ', 'yield ', 'yield # => ', 'throw ', 'include ', 'require_once ', 'fn() => ', 'new '] as $prefix) {
	$forms[] = $prefix . '#';
}
$forms[] = '#++';
$forms[] = '#--';
$forms[] = '#->m()';
$forms[] = '#[0]';
$expressions = [];
foreach ($forms as $outer) {
	$slots = substr_count($outer, '#');
	foreach ($forms as $inner) {
		foreach (array_unique([0, $slots - 1]) as $slot) {
			foreach ([$inner, "($inner)"] as $operand) {
				$parts = explode('#', $outer);
				$expression = '';
				foreach ($parts as $i => $part) {
					$expression .= $part . ($i === count($parts) - 1 ? '' : ($i === $slot ? $operand : '#'));
				}
				$expressions[$expression] = true;
			}
		}
	}
}
// nikic's error recovery never finishes on these, so neither does PHPStan
// (parity/known-deviations.md, prefix-incdec-new-hangs).
unset($expressions['++new #'], $expressions['--new #']);
foreach (array_keys($expressions) as $expression) {
	$name = 'a';
	$code = preg_replace_callback('/#/', static function () use (&$name): string {
		return '$' . $name++;
	}, (string) $expression);
	emit($store, 'operators/' . $code, "<?php\n\nfunction f()\n{\n\treturn $code;\n}\n");
}

fwrite(STDERR, "skipped: $skippedNikic nikic sections, $skippedPhpt php-src tests\n");
