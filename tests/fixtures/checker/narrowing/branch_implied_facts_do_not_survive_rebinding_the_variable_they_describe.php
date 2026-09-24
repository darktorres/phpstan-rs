<?php
            use function PHPStan\Testing\assertType;
            class Expr {}
            class Arr extends Expr {}
            class Arg {
                public Expr $value;
                public function __construct() { $this->value = new Expr(); }
            }
            class Call {
                /** @var array<Arg|int> */
                public array $args = [];
            }
            /** @param list<Call> $calls */
            function objects(array $calls): void {
                $seen = [];
                foreach ($calls as $call) {
                    if (! $call->args[0] instanceof Arg || ! $call->args[0]->value instanceof Arr) {
                        continue;
                    }
                    $seen[] = 1;
                }
            }
            /** @param list<array<string, mixed>> $rows */
            function flags(array $rows, bool $b): void {
                $out = [];
                foreach ($rows as $row) {
                    if ($b) {
                        $row['name'] = 'y';
                    } else {
                        assertType('mixed', $row['name']);
                        $row['name'] = strip_tags('x');
                    }
                    $out[] = $row;
                }
            }
        