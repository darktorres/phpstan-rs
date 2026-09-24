<?php
            class Base {}
            class Named extends Base { public function __toString(): string { return "n"; } }
            class Ident extends Base { public function __toString(): string { return "i"; } }
            function test(Base $node): string {
                return $node instanceof Named || $node instanceof Ident
                    ? (string) $node
                    : "";
            }
        