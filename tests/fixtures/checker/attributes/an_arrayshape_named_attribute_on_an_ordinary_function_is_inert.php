<?php
            #[\Attribute]
            class ArrayShape { public function __construct(array $shape) {} }
            /** @param string $x */
            function needsString($x): void {}
            #[ArrayShape(["red" => "int", "green" => "int"])]
            function pixel(): array {
                return ['red' => 1, 'green' => 2];
            }
            function f(): void {
                needsString(pixel()['red']);
            }
        