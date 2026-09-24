<?php declare(strict_types = 1);

// PHPUnit bootstrap for `cargo xtask parity`: loads PHPStan's own test
// bootstrap with `RuleTestCase::analyse()` and
// `TypeInferenceTestCase::gatherAssertTypes()` rewritten to record their
// arguments, as JSON lines appended to $PHPSTAN_RS_PARITY_RECORDS,
// instead of analysing anything.

final class PhpstanRsParity
{
	/** @var array<string, int> */
	private static array $calls = [];

	/**
	 * @param string[] $files
	 * @param list<array{0: string, 1: int, 2?: string|null}> $expectedErrors
	 */
	public static function rule(\PHPStan\Testing\RuleTestCase $test, string $rule, array $files, array $expectedErrors): void
	{
		$id = get_class($test) . '::' . $test->name() . ($test->dataName() !== '' ? '#' . $test->dataName() : '');
		self::$calls[$id] = (self::$calls[$id] ?? 0) + 1;
		self::write([
			'kind' => 'rule',
			'test' => $id . '@' . self::$calls[$id],
			'rule' => $rule,
			'files' => array_map(static fn (string $f): string => realpath($f) ?: $f, $files),
			'expected' => array_map(static fn (array $e): array => [
				'message' => $e[0],
				'line' => $e[1],
				'tip' => $e[2] ?? null,
			], $expectedErrors),
			'configs' => array_map(static fn (string $f): string => realpath($f) ?: $f, $test::getAdditionalConfigFiles()),
		]);
	}

	/** @param class-string<\PHPStan\Testing\TypeInferenceTestCase> $test */
	public static function types(string $test, string $file): void
	{
		self::write([
			'kind' => 'types',
			'test' => $test,
			'files' => [realpath($file) ?: $file],
			'configs' => array_map(static fn (string $f): string => realpath($f) ?: $f, $test::getAdditionalConfigFiles()),
		]);
	}

	/** @param array<string, mixed> $record */
	private static function write(array $record): void
	{
		file_put_contents(
			(string) getenv('PHPSTAN_RS_PARITY_RECORDS'),
			json_encode($record, JSON_THROW_ON_ERROR | JSON_UNESCAPED_SLASHES | JSON_UNESCAPED_UNICODE) . "\n",
			FILE_APPEND | LOCK_EX,
		);
	}
}

/**
 * The source of $file with the body of the method whose signature is
 * $signature replaced by $body.
 */
function phpstan_rs_parity_rewrite(string $file, string $signature, string $body): string
{
	$source = (string) file_get_contents($file);
	$start = strpos($source, $signature);
	if ($start === false) {
		throw new LogicException(sprintf('%s: no %s', $file, $signature));
	}
	$open = strpos($source, "\n\t{\n", $start);
	$close = $open === false ? false : strpos($source, "\n\t}\n", $open);
	if ($open === false || $close === false) {
		throw new LogicException(sprintf('%s: no body for %s', $file, $signature));
	}

	$rewritten = substr($source, 0, $open) . "\n\t{\n" . $body . substr($source, $close);

	// The copy is loaded from elsewhere: keep its paths the original's.
	return str_replace(
		['__DIR__', '__FILE__'],
		[var_export(dirname($file), true), var_export($file, true)],
		$rewritten,
	);
}

$phpstanSrc = dirname(__DIR__, 2) . '/vendor/phpstan-src';
$rewritten = [
	'PHPStan\\Testing\\RuleTestCase' => phpstan_rs_parity_rewrite(
		$phpstanSrc . '/src/Testing/RuleTestCase.php',
		'public function analyse(array $files, array $expectedErrors): void',
		"\t\t\\PhpstanRsParity::rule(\$this, get_class(\$this->getRule()), \$files, \$expectedErrors);\n\t\t\$this->addToAssertionCount(1);",
	),
	'PHPStan\\Testing\\TypeInferenceTestCase' => phpstan_rs_parity_rewrite(
		$phpstanSrc . '/src/Testing/TypeInferenceTestCase.php',
		'public static function gatherAssertTypes(string $file): array',
		"\t\t\\PhpstanRsParity::types(static::class, \$file);\n\t\treturn [];",
	),
];
spl_autoload_register(static function (string $class) use ($rewritten): void {
	if (!isset($rewritten[$class])) {
		return;
	}
	// The rewritten copy keeps the original's namespace and imports.
	$copy = sys_get_temp_dir() . '/phpstan-rs-parity-' . md5($rewritten[$class]) . '.php';
	if (!is_file($copy)) {
		file_put_contents($copy, $rewritten[$class]);
	}
	require $copy;
}, true, true);

require $phpstanSrc . '/tests/bootstrap.php';
