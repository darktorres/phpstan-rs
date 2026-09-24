<?php
            abstract class Base {
                abstract protected function write(): void;
            }
            class A extends Base {
                private ?B $b = null;
                protected function write(): void {
                    if ($this->b) {
                        $this->b->write();
                    }
                }
            }
            class B extends Base {
                protected function write(): void {}
            }
        