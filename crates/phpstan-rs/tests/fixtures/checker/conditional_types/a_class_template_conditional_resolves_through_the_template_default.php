<?php
            use function PHPStan\Testing\assertType;
            class ArrayNode {}
            class ScalarNode {}
            /** @template T of 'array'|'scalar' = 'array' */
            class Builder {
                /** @return (T is 'array' ? ArrayNode : ScalarNode) */
                public function root(): object { return new ArrayNode(); }
            }
            /** @param Builder<'scalar'> $s */
            function f(Builder $s): void {
                assertType('ArrayNode', (new Builder())->root());
                assertType('ScalarNode', $s->root());
            }
        