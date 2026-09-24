<?php
            trait T {
                abstract public function foo(): void;
            }
            class UsesT {
                use T;
                public function foo(): void {}
            }
        