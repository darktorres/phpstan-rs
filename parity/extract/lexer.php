<?php declare(strict_types = 1);

// The lexer suite's expected side (docs/design/scoreboard.md, "lexer").
//
// Usage: php lexer.php <phpstan-src> < inputs.tsv > cases.jsonl
//
// inputs.tsv holds one input per line: its id, a tab, and its file. Each
// input is lexed as it is and with its line breaks rewritten to "\r\n" and
// to "\r", the rewrites' ids ending in #crlf and #cr. One output line per
// case: its id, the token count, the SHA-256 of the canonical token stream
// and the lexer's errors.
//
// With --show <id> <file> it prints that case's canonical token stream; the
// file is the input's, before any rewrite.

use PhpParser\ErrorHandler\Collecting;
use PhpParser\Lexer;

$src = $argv[1] ?? throw new RuntimeException('usage: lexer.php <phpstan-src> [--show <id> <file>]');
require_once $src . '/vendor/autoload.php';

const VARIANTS = ['' => null, '#crlf' => "\r\n", '#cr' => "\r"];

/**
 * The canonical token stream: one token per line, "kind pos length line".
 *
 * @return array{string, int, list<array{message: string, pos: int|null}>}
 */
function canonical(string $code): array
{
	$errors = new Collecting();
	$tokens = (new Lexer())->tokenize($code, $errors);
	$out = '';
	foreach ($tokens as $token) {
		$kind = match (true) {
			$token->id === 0 => 'EOF',
			$token->id < 256 => chr($token->id),
			default => token_name($token->id),
		};
		$out .= $kind . ' ' . $token->pos . ' ' . strlen($token->text) . ' ' . $token->line . "\n";
	}
	$list = [];
	foreach ($errors->getErrors() as $error) {
		$attributes = $error->getAttributes();
		$list[] = ['message' => $error->getRawMessage(), 'pos' => $attributes['startFilePos'] ?? null];
	}
	return [$out, count($tokens), $list];
}

function variant(string $code, ?string $eol): string
{
	return $eol === null ? $code : (string) preg_replace('/\r\n|\r|\n/', $eol, $code);
}

if (($argv[2] ?? null) === '--show') {
	$id = $argv[3] ?? throw new RuntimeException('--show needs a case id and its file');
	$file = $argv[4] ?? throw new RuntimeException('--show needs a case id and its file');
	$eol = null;
	foreach (VARIANTS as $suffix => $variantEol) {
		if ($suffix !== '' && str_ends_with($id, $suffix)) {
			$eol = $variantEol;
		}
	}
	[$stream] = canonical(variant((string) file_get_contents($file), $eol));
	echo $stream;
	exit(0);
}

$flags = JSON_THROW_ON_ERROR | JSON_UNESCAPED_SLASHES | JSON_UNESCAPED_UNICODE | JSON_INVALID_UTF8_SUBSTITUTE;
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
	foreach (VARIANTS as $suffix => $eol) {
		[$stream, $count, $errors] = canonical(variant($code, $eol));
		echo json_encode([
			'id' => $id . $suffix,
			'tokens' => $count,
			'sha256' => hash('sha256', $stream),
			'errors' => $errors,
		], $flags), "\n";
	}
}
