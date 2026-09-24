<?php
            abstract class Shape {
                abstract public function area(): float;
            }
            function f() {
                new Shape();
            }
        