<?php

declare(strict_types=1);

use function PHPStan\Testing\assertType;

/** @param list<string> $plugins */
function f(array $plugins, string $installedOn): void
{
    $info = ['stats' => ['nb_photos' => 1, 'installed_on' => $installedOn, 'nb_private' => 0], 'plugins' => []];
    foreach ($plugins as $plugin) {
        if ($plugin === '') {
            $info['stats']['nb_private']++;
            continue;
        }
        $info['plugins'][] = $plugin;
    }
    assertType('array{nb_photos: 1, installed_on: string, nb_private: int<0, max>}', $info['stats']);
}
