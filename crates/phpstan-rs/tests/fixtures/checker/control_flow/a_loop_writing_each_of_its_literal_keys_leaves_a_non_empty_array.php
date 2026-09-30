<?php
use function PHPStan\Testing\assertType;

function filters(): void {
    $displayFilters = [];
    foreach (['words' => true, 'tags' => false] as $filtName => $filtConf) {
        $displayFilters[$filtName] = $filtConf;
    }
    assertType('array{words: bool, tags: bool}', $displayFilters);
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
    assertType("non-empty-array{'text/html'?: 'x', 'text/plain'?: 'x'}", $contents);
}

filters();
contents(true);
contents(false);
