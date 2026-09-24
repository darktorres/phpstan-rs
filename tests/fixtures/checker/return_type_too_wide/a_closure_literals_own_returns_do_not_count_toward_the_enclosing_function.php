<?php
            class C {
                private function f(): int|string {
                    $g = function () { return 'x'; };
                    return 1;
                }
                public function use(): int|string { return $this->f(); }
            }
        