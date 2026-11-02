<?php declare(strict_types = 1);

// PHPUnit bootstrap for the rules suite's extraction (docs/design/scoreboard.md).
//
// It puts a recording PHPStan\Testing\RuleTestCase in place of the real one,
// then loads phpstan-src's own bootstrap. Each analyse() or fix() call records
// what the test expects and returns without analysing anything: getRule() is
// never called. Records go, one JSON object per line, to $PARITY_OUT.
//
// Environment: PHPSTAN_SRC (the vendor/phpstan-src checkout), PARITY_OUT,
// PARITY_WORK (a directory for the generated class).

use PhpParser\Node;
use PhpParser\NodeTraverser;
use PhpParser\NodeVisitorAbstract;
use PhpParser\ParserFactory;
use PhpParser\PrettyPrinter\Standard;

$src = getenv('PHPSTAN_SRC') ?: throw new RuntimeException('PHPSTAN_SRC is not set');
$out = getenv('PARITY_OUT') ?: throw new RuntimeException('PARITY_OUT is not set');
$work = getenv('PARITY_WORK') ?: throw new RuntimeException('PARITY_WORK is not set');

require_once $src . '/vendor/autoload.php';

final class ParityRuleRecorder
{

	/** @var resource */
	private static $handle;

	private static string $root;

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
		$record = [
			'test' => $test::class,
			'method' => $test->name(),
			'data_set' => $test->dataName(),
			'kind' => $kind,
			'files' => array_map(self::relative(...), $files),
			'expected' => $kind === 'fix' ? self::relative((string) $expected) : $expected,
			'toggles' => self::toggles($test),
			'config_files' => array_map(self::relative(...), $test::getAdditionalConfigFiles()),
		];
		fwrite(self::$handle, json_encode($record, JSON_THROW_ON_ERROR | JSON_UNESCAPED_SLASHES | JSON_UNESCAPED_UNICODE | JSON_INVALID_UTF8_SUBSTITUTE) . "\n");
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

// Build the recording RuleTestCase from the real one: the same class, with the
// bodies of analyse(), fix() and gatherAnalyserErrors() replaced.
$generated = $work . '/RuleTestCase.php';
$parser = (new ParserFactory())->createForHostVersion();
$ast = $parser->parse((string) file_get_contents($src . '/src/Testing/RuleTestCase.php'));
$traverser = new NodeTraverser();
$traverser->addVisitor(new class ($parser, $src . '/src/Testing') extends NodeVisitorAbstract {

	public function __construct(private \PhpParser\Parser $parser, private string $testingDir)
	{
	}

	public function leaveNode(Node $node)
	{
		// The class is written elsewhere, so __DIR__ must keep meaning the
		// real file's directory.
		if ($node instanceof Node\Scalar\MagicConst\Dir) {
			return new Node\Scalar\String_($this->testingDir);
		}
		if (!$node instanceof Node\Stmt\ClassMethod) {
			return null;
		}
		$body = match ($node->name->toString()) {
			'analyse' => '\ParityRuleRecorder::record($this, "analyse", $files, $expectedErrors);',
			'fix' => '\ParityRuleRecorder::record($this, "fix", [$file], $expectedFile);',
			'gatherAnalyserErrors' => 'return [];',
			default => null,
		};
		if ($body === null) {
			return null;
		}
		$node->stmts = $this->parser->parse('<?php ' . $body);
		return $node;
	}

});
file_put_contents($generated, (new Standard())->prettyPrintFile($traverser->traverse($ast)));

ParityRuleRecorder::open($out, $src);
require_once $generated;
require_once $src . '/tests/bootstrap.php';
