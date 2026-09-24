<?php
            use function PHPStan\Testing\assertType;
            interface Meta {}
            class OrmMeta implements Meta { public function table(): string { return ''; } }
            /** @template T of Meta */
            interface Factory {
                /** @return list<T> */
                public function all(): array;
            }
            /**
             * @template M of Meta
             * @template-implements Factory<M>
             */
            abstract class BaseFactory implements Factory {
                /** {@inheritDoc} */
                public function all(): array { return []; }
            }
            /** @extends BaseFactory<OrmMeta> */
            class OrmFactory extends BaseFactory {}
            function f(OrmFactory $f): void {
                assertType('list<OrmMeta>', $f->all());
                foreach ($f->all() as $m) { $m->table(); }
            }
        