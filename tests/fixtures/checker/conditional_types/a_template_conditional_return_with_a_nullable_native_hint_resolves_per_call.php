<?php
    use function PHPStan\Testing\assertType;
    interface Pkg {}
    interface CompletePkg extends Pkg {}
    class Sorter {
        /** @template T of Pkg
         *  @param array<T> $packages
         *  @return ($packages is non-empty-array<T> ? T : T|null) */
        public static function most(array $packages): ?Pkg { return null; }
    }
    interface Repo {
        /** @return Pkg[] */ public function find(): array;
        /** @return non-empty-array<Pkg> */ public function findSome(): array;
        /** @return array<CompletePkg> */ public function findComplete(): array;
    }
    function f(Repo $r): void {
        assertType('Pkg|null', Sorter::most($r->find()));
        assertType('Pkg', Sorter::most($r->findSome()));
        assertType('CompletePkg|null', Sorter::most($r->findComplete()));
    }
