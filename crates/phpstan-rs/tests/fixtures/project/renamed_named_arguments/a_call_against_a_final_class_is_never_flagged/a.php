<?php
                final class Base {
                    public function f(int $old): void {}
                }
                function call_it(Base $b): void {
                    $b->f(old: 1);
                }
            