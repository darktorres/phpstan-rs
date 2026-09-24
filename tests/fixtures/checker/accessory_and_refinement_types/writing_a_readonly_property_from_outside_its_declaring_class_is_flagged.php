<?php
            class Point1 {
                public function __construct(public readonly int $x, public readonly int $y) {}
            }
            function mutateReadonly(Point1 $p): void { $p->x = 5; }
        