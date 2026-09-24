<?php
            class FakeFileInfo9921 {}
            class Iter {
                public function current() { return null; }
            }
            function f(Iter $it): void {
                $file = $it->current();
                /** @var FakeFileInfo9921 $file */
                if (true) {
                }
                $file->missingMethod();
            }
        