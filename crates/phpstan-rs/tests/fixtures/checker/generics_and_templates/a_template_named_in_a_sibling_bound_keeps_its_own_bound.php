<?php

declare(strict_types=1);

namespace Lib {
    /** @template T of object */
    class Repo {}
}

namespace App {
    use Lib\Repo;

    /**
     * @template E of object
     * @template R of Repo<E>
     * @param class-string<R> $class
     * @return R
     */
    function make(string $class): Repo
    {
        return new $class();
    }
}
