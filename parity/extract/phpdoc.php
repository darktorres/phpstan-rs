<?php declare(strict_types = 1);

// The phpdoc suite's expected side (docs/design/scoreboard.md, "phpdoc").
//
// Usage: php phpdoc.php <phpstan-src> --list < inputs.tsv > docs.tsv
//        php phpdoc.php <phpstan-src> < docs.tsv > cases.jsonl
//        php phpdoc.php <phpstan-src> --show <id> <file> <offset> <length>
//
// --list reads the parser suite's inputs (one id, a tab and a file per line)
// and prints every distinct doc comment once, at its first occurrence in
// that order: its case id (the input's id, "@" and the comment's byte
// offset), a tab, the file, the offset and the length, tab-separated.
//
// Without --list it reads those lines and parses each comment as PHPStan's
// PhpDocStringResolver does: phpdoc-parser's Lexer, then PhpDocParser with
// the ParserConfig of conf/services.neon (the lines attributes), then
// consumeTokenType(TOKEN_END). One output line per case: its id, the node
// count, the SHA-256 of the canonical tree, the SHA-256 of the printed tree
// ((string) of the PhpDocNode) and the message of the exception thrown, or
// null. With --show it prints that case's canonical tree, pretty-printed.
//
// The canonical tree:
// - a node is {"node": <class name without PHPStan\PhpDocParser\Ast\>,
//   <public properties in declared order>..., "attributes": {...}}, with
//   attribute keys sorted;
// - an InvalidTagValueNode also has "exception", its parser exception's
//   message;
// - a string that is not UTF-8 is {"bytes": "<hex>"};
// - a float is {"float": "<var_export>"}.

use PHPStan\PhpDocParser\Ast\Node;
use PHPStan\PhpDocParser\Ast\PhpDoc\InvalidTagValueNode;
use PHPStan\PhpDocParser\Lexer\Lexer;
use PHPStan\PhpDocParser\Parser\ConstExprParser;
use PHPStan\PhpDocParser\Parser\ParserException;
use PHPStan\PhpDocParser\Parser\PhpDocParser;
use PHPStan\PhpDocParser\Parser\TokenIterator;
use PHPStan\PhpDocParser\Parser\TypeParser;
use PHPStan\PhpDocParser\ParserConfig;

$src = $argv[1] ?? throw new RuntimeException('usage: phpdoc.php <phpstan-src> [--list | --show <id> <file> <offset> <length>]');
require_once $src . '/vendor/autoload.php';

const FLAGS = JSON_THROW_ON_ERROR | JSON_UNESCAPED_SLASHES | JSON_UNESCAPED_UNICODE;
const DEPTH = 2147483647;
const AST = 'PHPStan\\PhpDocParser\\Ast\\';

if (($argv[2] ?? null) === '--list') {
	$seen = [];
	while (($line = fgets(STDIN)) !== false) {
		$line = rtrim($line, "\n");
		if ($line === '') {
			continue;
		}
		[$id, $file] = explode("\t", $line, 2) + [1 => ''];
		$code = file_get_contents($file);
		if ($code === false) {
			throw new RuntimeException("cannot read $file for $id");
		}
		// Lexing raises php's own warnings for some inputs (an octal escape
		// past \377); they belong to the input, not to this script.
		foreach (@PhpToken::tokenize($code) as $token) {
			if ($token->id !== T_DOC_COMMENT || isset($seen[$token->text])) {
				continue;
			}
			$seen[$token->text] = true;
			echo $id, '@', $token->pos, "\t", $file, "\t", $token->pos, "\t", strlen($token->text), "\n";
		}
	}
	exit(0);
}

$config = new ParserConfig(['lines' => true]);
$constExprParser = new ConstExprParser($config);
$typeParser = new TypeParser($config, $constExprParser);
$lexer = new Lexer($config);
$parser = new PhpDocParser($config, $typeParser, $constExprParser);

/** @return array{Node|null, string|null} the tree and the exception's message */
function parse(Lexer $lexer, PhpDocParser $parser, string $doc): array
{
	$tokens = new TokenIterator($lexer->tokenize($doc));
	try {
		$node = $parser->parse($tokens);
		$tokens->consumeTokenType(Lexer::TOKEN_END);
		return [$node, null];
	} catch (ParserException $exception) {
		return [null, $exception->getMessage()];
	}
}

function attributes(Node $node): array
{
	$property = new ReflectionProperty($node, 'attributes');
	$out = $property->getValue($node);
	ksort($out, SORT_STRING);
	return $out;
}

function canonical(mixed $value, int &$count): mixed
{
	if ($value instanceof Node) {
		$count++;
		$class = get_class($value);
		$out = ['node' => str_starts_with($class, AST) ? substr($class, strlen(AST)) : $class];
		foreach ((new ReflectionObject($value))->getProperties(ReflectionProperty::IS_PUBLIC) as $property) {
			if ($property->isStatic()) {
				continue;
			}
			$out[$property->getName()] = canonical($property->getValue($value), $count);
		}
		if ($value instanceof InvalidTagValueNode) {
			$out['exception'] = canonical($value->exception->getMessage(), $count);
		}
		$out['attributes'] = (object) array_map(static fn (mixed $attribute): mixed => canonical($attribute, $count), attributes($value));
		return $out;
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
	if ($value instanceof UnitEnum) {
		return ['enum' => get_class($value) . '::' . $value->name];
	}
	if (is_object($value)) {
		throw new RuntimeException('unexpected ' . get_class($value) . ' in a PHPDoc tree');
	}
	return $value;
}

function comment(string $file, string $offset, string $length): string
{
	$doc = file_get_contents($file, false, null, (int) $offset, (int) $length);
	if ($doc === false || strlen($doc) !== (int) $length) {
		throw new RuntimeException("cannot read $length bytes at $offset of $file");
	}
	return $doc;
}

if (($argv[2] ?? null) === '--show') {
	[, , , $id, $file, $offset, $length] = $argv + array_fill(0, 7, null);
	if ($length === null) {
		throw new RuntimeException('--show needs a case id, its file, offset and length');
	}
	[$node, $error] = parse($lexer, $parser, comment($file, $offset, $length));
	$count = 0;
	echo json_encode($node === null ? ['exception' => $error] : canonical($node, $count), FLAGS | JSON_PRETTY_PRINT, DEPTH), "\n";
	exit(0);
}

while (($line = fgets(STDIN)) !== false) {
	$line = rtrim($line, "\n");
	if ($line === '') {
		continue;
	}
	[$id, $file, $offset, $length] = explode("\t", $line, 4) + [1 => '', 2 => '', 3 => ''];
	[$node, $error] = parse($lexer, $parser, comment($file, $offset, $length));
	$count = 0;
	$digest = $node === null ? null : hash('sha256', json_encode(canonical($node, $count), FLAGS, DEPTH));
	echo json_encode([
		'id' => $id,
		'nodes' => $count,
		'sha256' => $digest,
		'printed' => $node === null ? null : hash('sha256', (string) $node),
		'error' => $error,
	], FLAGS | JSON_INVALID_UTF8_SUBSTITUTE), "\n";
}
