<?php
            interface Repo92 {
                /**
                 * @template T of object
                 * @param class-string<T> $entityName
                 * @return T|null
                 */
                public function getReference(string $entityName, mixed $id): object|null;
            }
            function make(): Repo92 {
                return new class implements Repo92 {
                    public function getReference(string $entityName, mixed $id): object|null {
                        return null;
                    }
                };
            }
        