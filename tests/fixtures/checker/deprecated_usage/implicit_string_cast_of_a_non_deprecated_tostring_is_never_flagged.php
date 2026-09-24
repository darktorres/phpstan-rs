<?php
            class NiceThing {
                public function __toString(): string { return "y"; }
            }
            function f(NiceThing $n): void {
                echo (string) $n;
            }
        