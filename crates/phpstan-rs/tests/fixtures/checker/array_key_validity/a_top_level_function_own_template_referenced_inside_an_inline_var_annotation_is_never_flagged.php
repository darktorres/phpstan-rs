<?php
            namespace Psl\Dict;
            use Closure;
            /**
             * @template Tk of array-key
             * @template Tv
             * @param iterable<Tv> $values
             * @param (Closure(Tv): ?Tk) $keyFunc
             * @return array<Tk, list<Tv>>
             */
            function group_by(iterable $values, \Closure $keyFunc): array {
                $result = [];
                foreach ($values as $value) {
                    $key = $keyFunc($value);
                    /** @var Tk $key */
                    $result[$key] = $value;
                }
                return $result;
            }
        