<?php
            class Immutable {
                public function __construct(public readonly int $v) {}
                public function bump(): void { $this->v = $this->v + 1; }
            }
        