<?php declare(strict_types = 1);

// Runs PHPStan's rule and type-inference tests under the recording bootstrap
// (bootstrap.php) and prints what they expect, one JSON record per line,
// sorted (docs/design/scoreboard.md, "rules" and "types").
//
// Usage: PARITY_WORK=<dir> php tests.php <phpstan-src> > records.jsonl
//
// The tests are every non-abstract class under tests/PHPStan that descends
// from PHPStan\Testing\RuleTestCase or TypeInferenceTestCase, found by
// reading the class declarations, not by loading them. PHPUnit runs them in
// the default order with phpstan-src's own phpunit.xml settings; its output
// goes to <work>/phpunit.txt and its summary line to stderr.

use PhpParser\Node;
use PhpParser\NodeFinder;
use PhpParser\NodeTraverser;
use PhpParser\NodeVisitor\NameResolver;
use PhpParser\ParserFactory;

$src = realpath($argv[1] ?? '') ?: throw new RuntimeException('usage: tests.php <phpstan-src>');
$work = getenv('PARITY_WORK') ?: throw new RuntimeException('PARITY_WORK is not set');
require_once $src . '/vendor/autoload.php';

const BASES = ['phpstan\testing\ruletestcase', 'phpstan\testing\typeinferencetestcase'];

// Every class declared under tests/PHPStan: its parent, whether it is
// abstract, and its file.
$parser = (new ParserFactory())->createForHostVersion();
$finder = new NodeFinder();
$classes = [];
$files = new RecursiveIteratorIterator(new RecursiveDirectoryIterator($src . '/tests/PHPStan', FilesystemIterator::SKIP_DOTS));
foreach ($files as $file) {
	if (!str_ends_with($file->getFilename(), '.php')) {
		continue;
	}
	try {
		$ast = $parser->parse((string) file_get_contents($file->getPathname())) ?? [];
	} catch (PhpParser\Error) {
		continue;
	}
	// Some test data has deliberate name conflicts; they are not test classes.
	$ast = (new NodeTraverser(new NameResolver(new PhpParser\ErrorHandler\Collecting())))->traverse($ast);
	foreach ($finder->findInstanceOf($ast, Node\Stmt\Class_::class) as $class) {
		if ($class->namespacedName === null) {
			continue;
		}
		$classes[strtolower($class->namespacedName->toString())] = [
			'parent' => $class->extends === null ? null : strtolower($class->extends->toString()),
			'abstract' => $class->isAbstract(),
			'file' => $file->getPathname(),
		];
	}
}

$descends = static function (string $name) use ($classes): bool {
	for ($seen = []; $name !== null && !isset($seen[$name]); $name = $classes[$name]['parent'] ?? null) {
		if (in_array($name, BASES, true)) {
			return true;
		}
		$seen[$name] = true;
	}
	return false;
};
$tests = [];
foreach ($classes as $name => $class) {
	if (!$class['abstract'] && str_ends_with($class['file'], 'Test.php') && $descends($name)) {
		$tests[$class['file']] = true;
	}
}
$tests = array_keys($tests);
sort($tests, SORT_STRING);

// phpstan-src's phpunit.xml with this bootstrap, a private cache, the
// default order and the test list above.
if (!is_dir($work) && !mkdir($work, 0777, true)) {
	throw new RuntimeException("cannot create $work");
}
$config = new DOMDocument();
$config->load($src . '/phpunit.xml');
$root = $config->documentElement;
$root->setAttribute('xsi:noNamespaceSchemaLocation', $src . '/vendor/phpunit/phpunit/phpunit.xsd');
$root->setAttribute('bootstrap', __DIR__ . '/bootstrap.php');
$root->setAttribute('cacheDirectory', $work . '/phpunit-cache');
$root->setAttribute('executionOrder', 'default');
$root->setAttribute('colors', 'false');
foreach (iterator_to_array($root->getElementsByTagName('testsuites')) as $old) {
	$root->removeChild($old);
}
$suites = $config->createElement('testsuites');
$suite = $config->createElement('testsuite');
$suite->setAttribute('name', 'parity');
foreach ($tests as $test) {
	$suite->appendChild($config->createElement('file', $test));
}
$suites->appendChild($suite);
$root->insertBefore($suites, $root->getElementsByTagName('groups')->item(0));
$source = $root->getElementsByTagName('source')->item(0);
foreach (iterator_to_array($source?->getElementsByTagName('directory') ?? []) as $directory) {
	$directory->textContent = $src . '/' . $directory->textContent;
}
$config->save($work . '/phpunit.xml');

$records = $work . '/records.jsonl';
$process = proc_open(
	[PHP_BINARY, '-d', 'memory_limit=-1', $src . '/vendor/bin/phpunit', '-c', $work . '/phpunit.xml'],
	[0 => ['pipe', 'r'], 1 => ['file', $work . '/phpunit.txt', 'w'], 2 => ['redirect', 1]],
	$pipes,
	$src,
	['PHPSTAN_SRC' => $src, 'PARITY_OUT' => $records, 'PARITY_WORK' => $work] + getenv(),
);
if ($process === false) {
	throw new RuntimeException('cannot start phpunit');
}
fclose($pipes[0]);
$status = proc_close($process);
$output = (string) file_get_contents($work . '/phpunit.txt');
// The stand-ins make tests fail and err on purpose: an empty assert list
// fails, and a data provider built from one errs. PHPUnit then exits 1 or 2.
// A run without a summary line, or another status, is a failed extraction.
if (!in_array($status, [0, 1, 2], true) || preg_match('/^(OK .*|Tests: .*)$/m', $output, $summary) !== 1) {
	fwrite(STDERR, $output);
	throw new RuntimeException("phpunit exited with $status");
}
fwrite(STDERR, count($tests) . ' test classes; ' . $summary[1] . "\n");

$lines = file($records, FILE_IGNORE_NEW_LINES) ?: [];
sort($lines, SORT_STRING);
foreach ($lines as $line) {
	echo $line, "\n";
}
