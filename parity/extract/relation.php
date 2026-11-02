<?php declare(strict_types = 1);

// How a types case's answer changed between two scoreboard runs
// (docs/design/scoreboard.md, "Output").
//
// Usage: php relation.php <phpstan-src> < pairs.jsonl > relations.txt
//
// Each input line is a JSON array of two type strings, the old answer and
// the new one, as describe() prints them. Each output line is the new type's
// relation to the old one, as PHPStan's own types judge it: both strings are
// resolved by the TypeStringResolver of the container PHPStan's tests use,
// then compared by isSuperTypeOf both ways:
// - "equal": each is a supertype of the other;
// - "narrower": the old type is a supertype of the new one, not the reverse;
// - "wider": the reverse;
// - "incomparable": neither;
// - "unparsed": a string that does not parse as a type.
//
// The container's tmpDir is under $PARITY_WORK.

use PHPStan\PhpDoc\TypeStringResolver;
use PHPStan\Testing\PHPStanTestCase;
use PHPStan\Type\Type;

$src = $argv[1] ?? throw new RuntimeException('usage: relation.php <phpstan-src> < pairs.jsonl');
$work = getenv('PARITY_WORK') ?: throw new RuntimeException('PARITY_WORK is not set');
require_once $src . '/vendor/autoload.php';

if (!is_dir($work)) {
	mkdir($work, 0777, true);
}
// PHPStanTestCase puts its container's tmpDir under sys_get_temp_dir().
putenv('TMPDIR=' . $work);
$resolver = PHPStanTestCase::getContainer()->getByType(TypeStringResolver::class);

$resolve = static function (string $type) use ($resolver): ?Type {
	try {
		return $resolver->resolve($type);
	} catch (Throwable) {
		return null;
	}
};

while (($line = fgets(STDIN)) !== false) {
	if (trim($line) === '') {
		continue;
	}
	[$old, $new] = json_decode($line, true, flags: JSON_THROW_ON_ERROR);
	$oldType = $resolve($old);
	$newType = $resolve($new);
	if ($oldType === null || $newType === null) {
		echo "unparsed\n";
		continue;
	}
	$covers = $oldType->isSuperTypeOf($newType)->yes();
	$covered = $newType->isSuperTypeOf($oldType)->yes();
	echo match (true) {
		$covers && $covered => 'equal',
		$covers => 'narrower',
		$covered => 'wider',
		default => 'incomparable',
	}, "\n";
}
