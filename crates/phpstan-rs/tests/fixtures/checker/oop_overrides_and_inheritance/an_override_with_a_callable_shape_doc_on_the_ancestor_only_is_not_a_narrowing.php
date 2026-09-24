<?php
            interface ReadableCollection3 {
                /**
                 * @phpstan-param Closure(int, int):bool $p
                 */
                public function exists(Closure $p): bool;
            }
            class AbstractLazyCollection3 implements ReadableCollection3 {
                public function exists(Closure $p): bool { return true; }
            }
        