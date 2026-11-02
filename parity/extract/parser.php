<?php declare(strict_types = 1);

// The parser suite's expected side (docs/design/scoreboard.md, "parser").
//
// Usage: php parser.php <phpstan-src> < inputs.txt > cases.jsonl
//
// inputs.txt holds one path per line, relative to <phpstan-src>. Each input
// is parsed as PHPStan parses a file (SimpleParser and RichParser): nikic's
// Php8 parser over the emulative lexer with a collecting error handler, then
// nikic's NameResolver with preserveOriginalNames. One output line per input:
// its id, the node count, the SHA-256 of the canonical AST and the errors
// PHPStan would report as phpstan.parse.
//
// With --show <id> it prints that input's canonical AST, pretty-printed.
//
// The canonical AST is nikic's JSON shape made total and stable:
// - a node is {"nodeType": ..., <sub-nodes in nikic's declared order>...,
//   "attributes": {...}}, with attribute keys sorted and the token
//   positions (startTokenPos, endTokenPos, a comment's tokenPos and
//   endTokenPos) left out;
// - a string that is not UTF-8 is {"bytes": "<hex>"};
// - a float is {"float": "<var_export>"}, which also covers INF and NAN.

use PhpParser\Comment;
use PhpParser\Error;
use PhpParser\ErrorHandler\Collecting;
use PhpParser\Lexer\Emulative;
use PhpParser\Node;
use PhpParser\NodeTraverser;
use PhpParser\NodeVisitor\NameResolver;
use PhpParser\Parser\Php8;

$src = $argv[1] ?? throw new RuntimeException('usage: parser.php <phpstan-src> [--show <id>]');
require_once $src . '/vendor/autoload.php';

const FLAGS = JSON_THROW_ON_ERROR | JSON_UNESCAPED_SLASHES | JSON_UNESCAPED_UNICODE;
// The test data nests deeper than json_encode's default limit of 512; the
// depth is a C int, so PHP_INT_MAX would overflow it.
const DEPTH = 2147483647;

/**
 * @return array{list<Node\Stmt>|null, list<array{message: string, line: int|null}>}
 */
function parse(string $code): array
{
	$errors = new Collecting();
	$nodes = (new Php8(new Emulative()))->parse($code, $errors);
	if ($errors->hasErrors()) {
		return [null, array_map(report(...), $errors->getErrors())];
	}
	$traverser = new NodeTraverser(new NameResolver(null, ['preserveOriginalNames' => true]));
	try {
		return [$traverser->traverse($nodes ?? []), []];
	} catch (Error $error) {
		return [null, [report($error)]];
	}
}

/** @return array{message: string, line: int|null} */
function report(Error $error): array
{
	// FileAnalyser reports getMessage(), on getStartLine() unless the line is -1.
	return ['message' => $error->getMessage(), 'line' => $error->getStartLine() !== -1 ? $error->getStartLine() : null];
}

function canonical(mixed $value, int &$count): mixed
{
	if ($value instanceof Node) {
		$count++;
		$out = ['nodeType' => $value->getType()];
		foreach ($value->getSubNodeNames() as $name) {
			$out[$name] = canonical($value->$name, $count);
		}
		$attributes = [];
		foreach ($value->getAttributes() as $key => $attribute) {
			if ($key === 'startTokenPos' || $key === 'endTokenPos') {
				continue;
			}
			$attributes[$key] = canonical($attribute, $count);
		}
		ksort($attributes, SORT_STRING);
		$out['attributes'] = (object) $attributes;
		return $out;
	}
	if ($value instanceof Comment) {
		return [
			'nodeType' => $value instanceof Comment\Doc ? 'Comment_Doc' : 'Comment',
			'text' => canonical($value->getText(), $count),
			'line' => $value->getStartLine(),
			'filePos' => $value->getStartFilePos(),
			'endLine' => $value->getEndLine(),
			'endFilePos' => $value->getEndFilePos(),
		];
	}
	if (is_array($value)) {
		$out = [];
		foreach ($value as $key => $item) {
			$out[$key] = canonical($item, $count);
		}
		return array_is_list($out) ? $out : (object) $out;
	}
	if (is_string($value)) {
		return mb_check_encoding($value, 'UTF-8') ? $value : ['bytes' => bin2hex($value)];
	}
	if (is_float($value)) {
		return ['float' => var_export($value, true)];
	}
	if (is_object($value)) {
		throw new RuntimeException('unexpected ' . get_class($value) . ' in the AST');
	}
	return $value;
}

if (($argv[2] ?? null) === '--show') {
	$id = $argv[3] ?? throw new RuntimeException('--show needs a case id');
	[$nodes, $errors] = parse((string) file_get_contents($src . '/' . $id));
	$count = 0;
	echo json_encode($nodes === null ? ['errors' => $errors] : canonical($nodes, $count), FLAGS | JSON_PRETTY_PRINT, DEPTH), "\n";
	exit(0);
}

while (($path = fgets(STDIN)) !== false) {
	$path = rtrim($path, "\n");
	if ($path === '') {
		continue;
	}
	$code = file_get_contents($src . '/' . $path);
	if ($code === false) {
		throw new RuntimeException('cannot read ' . $path);
	}
	[$nodes, $errors] = parse($code);
	$count = 0;
	$digest = $nodes === null ? null : hash('sha256', json_encode(canonical($nodes, $count), FLAGS, DEPTH));
	echo json_encode([
		'id' => $path,
		'nodes' => $count,
		'sha256' => $digest,
		'errors' => $errors,
	], FLAGS | JSON_INVALID_UTF8_SUBSTITUTE), "\n";
}
