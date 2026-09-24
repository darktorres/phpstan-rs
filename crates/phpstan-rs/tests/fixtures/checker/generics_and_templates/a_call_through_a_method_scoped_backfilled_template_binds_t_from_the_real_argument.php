<?php
            use function PHPStan\Testing\assertType;
            class Foo14 {}
            interface Repo14 {
                /**
                 * @template T of object
                 * @param class-string<T> $entityName
                 * @return T|null
                 */
                public function getReference(string $entityName, mixed $id): object|null;
            }
            class ConcreteRepo14 implements Repo14 {
                public function getReference(string $entityName, mixed $id): object|null { return null; }
            }
            function f(ConcreteRepo14 $repo): void {
                assertType('Foo14|null', $repo->getReference(Foo14::class, 1));
            }
        