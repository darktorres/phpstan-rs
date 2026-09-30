<?php
use function PHPStan\Testing\assertType;

function filters(): void {
    $displayFilters = [];
    foreach (['words' => true, 'tags' => false] as $filtName => $filtConf) {
        $displayFilters[$filtName] = $filtConf;
    }
    assertType('array{tags?: bool, words: bool}|array{tags: bool, words?: bool}', $displayFilters);
}

function contents(bool $html): void {
    $list = [];
    if ($html) {
        $list[] = 'text/html';
    }
    $list[] = 'text/plain';
    $contents = [];
    foreach ($list as $contentType) {
        $contents[$contentType] = 'x';
    }
    assertType("array{'text/plain'?: 'x', 'text/html': 'x'}|array{'text/plain': 'x', 'text/html'?: 'x'}", $contents);
}

filters();
contents(true);
contents(false);
