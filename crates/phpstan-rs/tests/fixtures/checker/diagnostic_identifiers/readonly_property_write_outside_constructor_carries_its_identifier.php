<?php
            class Point {
                public function __construct(public readonly int $x) {}
                public function moveTo(int $newX): void {
                    $this->x = $newX;
                }
            }
        