<?php
            use function PHPStan\Testing\assertType;
            class Holder {
                /** @var int|string */
                public int $count = 0;
                public function f(): void { assertType('int', $this->count); }
            }
        