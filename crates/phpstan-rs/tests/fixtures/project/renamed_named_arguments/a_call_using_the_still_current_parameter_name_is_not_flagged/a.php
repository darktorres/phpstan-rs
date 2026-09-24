<?php
                class Base {
                    public function f(int $x): void {}
                }
                class Sub extends Base {
                    public function f(int $x): void {}
                }
                function call_it(Base $b): void {
                    $b->f(x: 1);
                }
            