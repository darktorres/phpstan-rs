<?php declare(strict_types = 1);

// PHPUnit bootstrap for the scoreboard's extraction (docs/design/scoreboard.md).
//
// It puts recording versions of PHPStan\Testing\RuleTestCase and
// PHPStan\Testing\TypeInferenceTestCase in place of the real ones, then loads
// phpstan-src's own bootstrap. Running a test then records what it expects,
// without analysing anything:
// - RuleTestCase: each analyse() or fix() call records its files and expected
//   errors; getRule() is never called;
// - TypeInferenceTestCase: each gatherAssertTypes() call records the file whose
//   asserts the test checks, and returns no asserts.
// Records go, one JSON object per line, to $PARITY_OUT.
//
// Environment: PHPSTAN_SRC (the vendor/phpstan-src checkout), PARITY_OUT,
// PARITY_WORK (a directory for the generated classes).

use PhpParser\Node;
use PhpParser\NodeTraverser;
use PhpParser\NodeVisitorAbstract;
use PhpParser\ParserFactory;
use PhpParser\PrettyPrinter\Standard;

$src = getenv('PHPSTAN_SRC') ?: throw new RuntimeException('PHPSTAN_SRC is not set');
$out = getenv('PARITY_OUT') ?: throw new RuntimeException('PARITY_OUT is not set');
$work = getenv('PARITY_WORK') ?: throw new RuntimeException('PARITY_WORK is not set');

require_once $src . '/vendor/autoload.php';

final class ParityRecorder
{

	/** @var resource */
	private static $handle;

	private static string $root;

	/** @var array<string, int> analyse() and fix() calls so far, per test */
	private static array $calls = [];

	public static function open(string $out, string $root): void
	{
		self::$handle = fopen($out, 'wb') ?: throw new RuntimeException("cannot write $out");
		self::$root = rtrim(realpath($root) ?: $root, '/') . '/';
	}

	/**
	 * @param list<string> $files
	 */
	public static function record(object $test, string $kind, array $files, mixed $expected): void
	{
		// A test method can call analyse() more than once; the ordinal tells
		// the calls apart in the case id.
		$call = self::$calls[$test::class . '::' . $test->name() . '#' . $test->dataName()] ??= 0;
		self::$calls[$test::class . '::' . $test->name() . '#' . $test->dataName()]++;
		$record = [
			'test' => $test::class,
			'kind' => $kind,
			'method' => $test->name(),
			'data_set' => $test->dataName(),
			'call' => $call,
			'files' => array_map(self::relative(...), $files),
			'expected' => $kind === 'fix' ? self::relative((string) $expected) : $expected,
			'toggles' => self::toggles($test),
			'config_files' => array_map(self::relative(...), $test::getAdditionalConfigFiles()),
		];
		fwrite(self::$handle, json_encode($record, JSON_THROW_ON_ERROR | JSON_UNESCAPED_SLASHES | JSON_UNESCAPED_UNICODE | JSON_INVALID_UTF8_SUBSTITUTE) . "\n");
	}

	/**
	 * @param class-string $testClass
	 */
	public static function recordTypeFile(string $testClass, string $file): void
	{
		$record = [
			'test' => $testClass,
			'kind' => 'types',
			'files' => [self::relative($file)],
			'config_files' => array_map(self::relative(...), $testClass::getAdditionalConfigFiles()),
		];
		fwrite(self::$handle, json_encode($record, JSON_THROW_ON_ERROR | JSON_UNESCAPED_SLASHES | JSON_UNESCAPED_UNICODE) . "\n");
	}

	private static function relative(string $path): string
	{
		$real = realpath($path);
		$path = $real === false ? $path : $real;
		return str_starts_with($path, self::$root) ? substr($path, strlen(self::$root)) : $path;
	}

	/**
	 * The scalar properties the test class itself declares: the toggles its
	 * getRule() reads, as they stand when analyse() is called.
	 *
	 * @return array<string, scalar|null|array<mixed>>
	 */
	private static function toggles(object $test): array
	{
		$toggles = [];
		$class = new ReflectionClass($test);
		while ($class !== false && $class->getName() !== 'PHPStan\Testing\RuleTestCase') {
			foreach ($class->getProperties() as $property) {
				if ($property->getDeclaringClass()->getName() !== $class->getName() || $property->isStatic()) {
					continue;
				}
				if (!$property->isInitialized($test)) {
					continue;
				}
				$value = $property->getValue($test);
				if (is_scalar($value) || $value === null || (is_array($value) && self::isPlain($value))) {
					$toggles[$property->getName()] ??= $value;
				}
			}
			$class = $class->getParentClass();
		}
		ksort($toggles);
		return $toggles;
	}

	/** @param array<mixed> $value */
	private static function isPlain(array $value): bool
	{
		foreach ($value as $item) {
			if (!(is_scalar($item) || $item === null || (is_array($item) && self::isPlain($item)))) {
				return false;
			}
		}
		return true;
	}

}

// Build each recording class from the real one: the same class, with some
// method bodies replaced. The classes are written elsewhere, so __DIR__ is
// replaced too, to keep meaning the real file's directory.
$parser = (new ParserFactory())->createForHostVersion();
$standIns = [
	'RuleTestCase' => [
		'analyse' => '\ParityRecorder::record($this, "analyse", $files, $expectedErrors);',
		'fix' => '\ParityRecorder::record($this, "fix", [$file], $expectedFile);',
		'gatherAnalyserErrors' => 'return [];',
	],
	'TypeInferenceTestCase' => [
		'gatherAssertTypes' => '\ParityRecorder::recordTypeFile(static::class, $file); return [];',
	],
];
$generated = [];
foreach ($standIns as $class => $bodies) {
	$ast = $parser->parse((string) file_get_contents($src . '/src/Testing/' . $class . '.php'));
	$traverser = new NodeTraverser();
	$traverser->addVisitor(new class ($parser, $src . '/src/Testing', $bodies) extends NodeVisitorAbstract {

		/** @param array<string, string> $bodies */
		public function __construct(private \PhpParser\Parser $parser, private string $testingDir, private array $bodies)
		{
		}

		public function leaveNode(Node $node)
		{
			if ($node instanceof Node\Scalar\MagicConst\Dir) {
				return new Node\Scalar\String_($this->testingDir);
			}
			if (!$node instanceof Node\Stmt\ClassMethod || !isset($this->bodies[$node->name->toString()])) {
				return null;
			}
			$node->stmts = $this->parser->parse('<?php ' . $this->bodies[$node->name->toString()]);
			return $node;
		}

	});
	$generated[] = $path = $work . '/' . $class . '.php';
	file_put_contents($path, (new Standard())->prettyPrintFile($traverser->traverse($ast)));
}

ParityRecorder::open($out, $src);
foreach ($generated as $path) {
	require_once $path;
}
require_once $src . '/tests/bootstrap.php';
