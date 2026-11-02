<?php declare(strict_types = 1);

// The phpdoc suite's expected side (docs/design/scoreboard.md, "phpdoc").
//
// Usage: php phpdoc.php <phpstan-src> --list <phpdoc-parser> <store> < inputs.tsv > docs.tsv
//        php phpdoc.php <phpstan-src> < docs.tsv > cases.jsonl
//        php phpdoc.php <phpstan-src> --show <id> <kind> <file> <offset> <length>
//
// --list prints the suite's cases, one per line: the case id, the kind
// ("doc" or "type"), the file, the byte offset and the length of the text,
// tab-separated. The cases are:
// - every distinct doc comment of the parser suite's inputs (read on stdin,
//   one id, a tab and a file per line), at its first occurrence in that
//   order. The id is the input's id, "@" and the comment's offset;
// - the inputs of phpdoc-parser's own parser tests, from the data providers
//   of PhpDocParserTest (doc comments) and TypeParserTest (types), written
//   to <store>/<sha256>.txt. The id is phpdoc-parser/<test>::<provider>#<key>.
//
// Without --list it reads those lines and parses each text as PHPStan does,
// with phpdoc-parser's Lexer and the ParserConfig of conf/services.neon (the
// lines attributes): a doc as PhpDocStringResolver does (PhpDocParser), a
// type as TypeStringResolver does (TypeParser), then
// consumeTokenType(TOKEN_END). One output line per case: its id, the node
// count, the SHA-256 of the canonical tree, the SHA-256 of the printed tree
// ((string) of the root node) and the message of the exception thrown, or
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

$src = $argv[1] ?? throw new RuntimeException('usage: phpdoc.php <phpstan-src> [--list <phpdoc-parser> <store> | --show <id> <kind> <file> <offset> <length>]');
require_once $src . '/vendor/autoload.php';

const FLAGS = JSON_THROW_ON_ERROR | JSON_UNESCAPED_SLASHES | JSON_UNESCAPED_UNICODE;
const DEPTH = 2147483647;
const AST = 'PHPStan\\PhpDocParser\\Ast\\';

if (($argv[2] ?? null) === '--list') {
	[, , , $repository, $store] = $argv + array_fill(0, 5, null);
	if ($store === null) {
		throw new RuntimeException('--list needs the phpdoc-parser repository and a store');
	}
	if (!is_dir($store) && !mkdir($store, 0777, true)) {
		throw new RuntimeException("cannot create $store");
	}
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
			echo $id, '@', $token->pos, "\tdoc\t", $file, "\t", $token->pos, "\t", strlen($token->text), "\n";
		}
	}
	foreach (providedInputs($repository) as $id => [$kind, $text]) {
		$path = $store . '/' . hash('sha256', $text) . '.txt';
		if (!is_file($path)) {
			file_put_contents($path . '.tmp', $text);
			rename($path . '.tmp', $path);
		}
		echo $id, "\t", $kind, "\t", $path, "\t0\t", strlen($text), "\n";
	}
	exit(0);
}

/**
 * The `$input` or `$phpDoc` argument of every data set of the parser tests'
 * data providers, by case id. A PhpDocParserTest input that is not a whole
 * doc comment (testParseTagValue's tag values) is not something PHPStan
 * parses, so it is left out.
 *
 * @return array<string, array{string, string}> the kind and the text
 */
function providedInputs(string $repository): array
{
	$tests = $repository . '/tests/PHPStan/Parser';
	foreach (glob($tests . '/Doctrine/*.php') ?: [] as $file) {
		require_once $file;
	}
	require_once $tests . '/NodeCollectingVisitor.php';
	$out = [];
	foreach (['PhpDocParserTest' => 'doc', 'TypeParserTest' => 'type'] as $short => $kind) {
		require_once "$tests/$short.php";
		$class = 'PHPStan\\PhpDocParser\\Parser\\' . $short;
		foreach ((new ReflectionClass($class))->getMethods() as $method) {
			if (!str_starts_with($method->getName(), 'test')) {
				continue;
			}
			$position = null;
			foreach ($method->getParameters() as $parameter) {
				if (in_array($parameter->getName(), ['input', 'phpDoc'], true)) {
					$position = $parameter->getPosition();
				}
			}
			preg_match_all('/@dataProvider\s+(\w+)/', (string) $method->getDocComment(), $providers);
			foreach ($position === null ? [] : $providers[1] as $provider) {
				foreach ((new $class($method->getName()))->$provider() as $key => $data) {
					$text = $data[$position];
					if ($kind === 'doc' && !str_starts_with($text, '/**')) {
						continue;
					}
					$out["phpdoc-parser/$short::$provider#$key"] = [$kind, $text];
				}
			}
		}
	}
	ksort($out, SORT_STRING);
	return $out;
}

$config = new ParserConfig(['lines' => true]);
$constExprParser = new ConstExprParser($config);
$typeParser = new TypeParser($config, $constExprParser);
$lexer = new Lexer($config);
$parser = new PhpDocParser($config, $typeParser, $constExprParser);

/** @return array{Node|null, string|null} the tree and the exception's message */
function parse(Lexer $lexer, PhpDocParser|TypeParser $parser, string $text): array
{
	$tokens = new TokenIterator($lexer->tokenize($text));
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

function parserFor(string $kind, PhpDocParser $phpDocParser, TypeParser $typeParser): PhpDocParser|TypeParser
{
	return match ($kind) {
		'doc' => $phpDocParser,
		'type' => $typeParser,
		default => throw new RuntimeException("unknown kind $kind"),
	};
}

if (($argv[2] ?? null) === '--show') {
	[, , , $id, $kind, $file, $offset, $length] = $argv + array_fill(0, 8, null);
	if ($length === null) {
		throw new RuntimeException('--show needs a case id, its kind, file, offset and length');
	}
	[$node, $error] = parse($lexer, parserFor($kind, $parser, $typeParser), comment($file, $offset, $length));
	$count = 0;
	echo json_encode($node === null ? ['exception' => $error] : canonical($node, $count), FLAGS | JSON_PRETTY_PRINT, DEPTH), "\n";
	exit(0);
}

while (($line = fgets(STDIN)) !== false) {
	$line = rtrim($line, "\n");
	if ($line === '') {
		continue;
	}
	[$id, $kind, $file, $offset, $length] = explode("\t", $line, 5) + [1 => '', 2 => '', 3 => '', 4 => ''];
	[$node, $error] = parse($lexer, parserFor($kind, $parser, $typeParser), comment($file, $offset, $length));
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
