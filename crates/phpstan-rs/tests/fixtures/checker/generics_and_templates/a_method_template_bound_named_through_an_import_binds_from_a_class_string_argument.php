<?php

declare(strict_types=1);

namespace Lib {
    /** @template T */
    class Repo {}

    /** @extends Repo<\stdClass> */
    class Special extends Repo {}
}

namespace App\Db {
    use Lib\Repo;

    final class Typed
    {
        /**
         * @template E of object
         * @template R of Repo<E>
         * @param Repo<E> $repository
         * @param class-string<R> $repositoryClass
         * @return R
         */
        public static function narrow(Repo $repository, string $repositoryClass): Repo
        {
            assert($repository instanceof $repositoryClass);

            return $repository;
        }
    }
}

namespace App {
    use App\Db\Typed;
    use Lib\Repo;
    use Lib\Special;
    use function PHPStan\Testing\assertType;

    /** @param Repo<\stdClass> $repository */
    function f(Repo $repository): void
    {
        assertType('Lib\Special', Typed::narrow($repository, Special::class));
    }
}
