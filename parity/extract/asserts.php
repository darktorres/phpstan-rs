<?php declare(strict_types = 1);

// Reads the type asserts of each file the types extraction recorded
// (docs/design/scoreboard.md, "types").
//
// Usage: php asserts.php <phpstan-src> < records.jsonl > asserts.jsonl
//
// records.jsonl is tests.php's output; its "types" records name the files.
//
// One output line per assertType, assertNativeType, assertSuperType or
// assertVariableCertainty call, with the call's line, its kind, the expected
// value and the asserted expression's source text, as PHPStan's
// TypeInferenceTestCase::gatherAssertTypes reads them.

use PhpParser\ConstExprEvaluationException;
use PhpParser\ConstExprEvaluator;
use PhpParser\Node;
use PhpParser\NodeFinder;
use PhpParser\NodeTraverser;
use PhpParser\NodeVisitor\NameResolver;
use PhpParser\NodeVisitor\ParentConnectingVisitor;
use PhpParser\ParserFactory;

$src = $argv[1] ?? throw new RuntimeException('usage: asserts.php <phpstan-src> < records.jsonl');
require_once $src . '/vendor/autoload.php';

$kinds = [
	'PHPStan\\Testing\\assertType' => 'type',
	'PHPStan\\Testing\\assertNativeType' => 'nativeType',
	'PHPStan\\Testing\\assertSuperType' => 'superType',
	'PHPStan\\Testing\\assertVariableCertainty' => 'variableCertainty',
];

/** @var array<string, list<string>> $files file => test classes */
$files = [];
foreach (file('php://stdin', FILE_IGNORE_NEW_LINES) ?: [] as $line) {
	$case = json_decode($line, true, flags: JSON_THROW_ON_ERROR);
	if ($case['kind'] !== 'types') {
		continue;
	}
	$files[$case['files'][0]][] = $case['test'];
}
ksort($files);

$parser = (new ParserFactory())->createForHostVersion();
$finder = new NodeFinder();
$flags = JSON_THROW_ON_ERROR | JSON_UNESCAPED_SLASHES | JSON_UNESCAPED_UNICODE | JSON_INVALID_UTF8_SUBSTITUTE;

foreach ($files as $file => $tests) {
	$code = (string) file_get_contents($src . '/' . $file);
	try {
		$ast = $parser->parse($code) ?? [];
	} catch (\PhpParser\Error $error) {
		echo json_encode(['file' => $file, 'error' => $error->getMessage()], $flags), "\n";
		continue;
	}
	$traverser = new NodeTraverser(new NameResolver(), new ParentConnectingVisitor());
	$ast = $traverser->traverse($ast);
	// PHPStan reads a non-literal expected string through the scope, as a
	// constant string. Here that is folded statically: `X::class` (with
	// self, static and parent taken from the enclosing class), class and
	// global constants declared in the same file, concatenation, sprintf
	// over constant arguments, an assignment's value, and a variable
	// assigned exactly once in the enclosing function.
	$constants = [];
	foreach ($finder->findInstanceOf($ast, Node\Stmt\ClassLike::class) as $classLike) {
		if ($classLike->namespacedName === null) {
			continue;
		}
		foreach ($classLike->getConstants() as $classConst) {
			foreach ($classConst->consts as $const) {
				$constants['class:' . strtolower($classLike->namespacedName->toString()) . '::' . $const->name->toString()] = $const->value;
			}
		}
	}
	foreach ($finder->findInstanceOf($ast, Node\Stmt\Const_::class) as $constStmt) {
		foreach ($constStmt->consts as $const) {
			$constants['global:' . strtolower((string) $const->namespacedName)] = $const->value;
		}
	}
	$enclosing = static function (Node $node, string $type): ?Node {
		for ($parent = $node->getAttribute('parent'); $parent !== null; $parent = $parent->getAttribute('parent')) {
			if ($parent instanceof $type) {
				return $parent;
			}
		}
		return null;
	};
	$className = static function (Node\Name $name, Node $at) use ($enclosing): ?string {
		$lower = $name->toLowerString();
		if (!in_array($lower, ['self', 'static', 'parent'], true)) {
			return $name->toString();
		}
		$class = $enclosing($at, Node\Stmt\ClassLike::class);
		if (!$class instanceof Node\Stmt\ClassLike || $class->namespacedName === null) {
			return null;
		}
		if ($lower !== 'parent') {
			return $class->namespacedName->toString();
		}
		return $class instanceof Node\Stmt\Class_ && $class->extends !== null ? $class->extends->toString() : null;
	};
	$evaluator = null;
	$evaluator = new ConstExprEvaluator(static function (Node\Expr $expr) use (&$evaluator, $constants, $className, $enclosing, $finder): mixed {
		if ($expr instanceof Node\Expr\ClassConstFetch && $expr->class instanceof Node\Name && $expr->name instanceof Node\Identifier) {
			$class = $className($expr->class, $expr);
			if ($class !== null && $expr->name->toLowerString() === 'class') {
				return $class;
			}
			$value = $class === null ? null : ($constants['class:' . strtolower($class) . '::' . $expr->name->toString()] ?? null);
			if ($value !== null) {
				return $evaluator->evaluateDirectly($value);
			}
		}
		if ($expr instanceof Node\Expr\ConstFetch) {
			$value = $constants['global:' . strtolower($expr->name->toString())] ?? null;
			if ($value !== null) {
				return $evaluator->evaluateDirectly($value);
			}
		}
		if ($expr instanceof Node\Expr\FuncCall && $expr->name instanceof Node\Name && $expr->name->toLowerString() === 'sprintf') {
			return sprintf(...array_map(static fn (Node\Arg $arg): mixed => $evaluator->evaluateDirectly($arg->value), $expr->getArgs()));
		}
		if ($expr instanceof Node\Expr\Assign) {
			return $evaluator->evaluateDirectly($expr->expr);
		}
		if ($expr instanceof Node\Expr\Variable && is_string($expr->name)) {
			$scope = $enclosing($expr, Node\FunctionLike::class);
			$assigns = $scope === null ? [] : $finder->find($scope->getStmts() ?? [], static fn (Node $node): bool => $node instanceof Node\Expr\Assign
				&& $node->var instanceof Node\Expr\Variable
				&& $node->var->name === $expr->name);
			if (count($assigns) === 1) {
				return $evaluator->evaluateDirectly($assigns[0]->expr);
			}
		}
		throw new ConstExprEvaluationException('not a constant string');
	});
	// 1-based byte column, so two asserts on one line get distinct case ids.
	$column = static function (int $pos) use ($code): int {
		$lineStart = $pos === 0 ? false : strrpos($code, "\n", $pos - strlen($code) - 1);
		return $pos - ($lineStart === false ? -1 : $lineStart);
	};
	$calls = $finder->find($ast, static fn (Node $node): bool => $node instanceof Node\Expr\FuncCall && $node->name instanceof Node\Name);
	foreach ($calls as $call) {
		$name = $call->name->getAttribute('namespacedName') ?? $call->name;
		$resolved = $call->name->getAttribute('resolvedName') ?? $name;
		$kind = $kinds[$resolved->toString()] ?? $kinds[ltrim($name->toString(), '\\')] ?? null;
		if ($kind === null) {
			continue;
		}
		$args = $call->getArgs();
		$expected = null;
		if ($kind === 'variableCertainty') {
			$certainty = $args[0]->value ?? null;
			if ($certainty instanceof Node\Expr\StaticCall && $certainty->name instanceof Node\Identifier) {
				$expected = $certainty->name->toString();
			}
		} elseif (isset($args[0])) {
			try {
				$value = $evaluator->evaluateDirectly($args[0]->value);
				$expected = is_string($value) ? $value : null;
			} catch (ConstExprEvaluationException) {
				$expected = null;
			}
		}
		$subject = $args[1]->value ?? null;
		echo json_encode([
			'file' => $file,
			'line' => $call->getStartLine(),
			'column' => $column($call->getStartFilePos()),
			'kind' => $kind,
			'expected' => $expected,
			'expr' => $subject === null ? null : substr($code, $subject->getStartFilePos(), $subject->getEndFilePos() - $subject->getStartFilePos() + 1),
			'tests' => array_values(array_unique($tests)),
		], $flags), "\n";
	}
}
