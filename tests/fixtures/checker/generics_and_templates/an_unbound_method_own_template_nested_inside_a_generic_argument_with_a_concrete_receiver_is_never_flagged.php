<?php
            /**
             * @template T
             */
            class EntityRepository {}
            class ConcreteEntity {}
            class TypedRepository {
                /**
                 * @template E of object
                 * @param EntityRepository<E> $repository
                 */
                public static function narrow($repository): void {}
            }
            /** @param EntityRepository<ConcreteEntity> $r */
            function f($r) {
                TypedRepository::narrow($r);
            }
        