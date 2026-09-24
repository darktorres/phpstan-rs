<?php
            class FakeFileInfo9921 {}
            class Iter {
                public function current() { return null; }
            }
            function f(Iter $it): string {
                /** @var FakeFileInfo9921 */
                $file = $it->current();
                return $file;
            }
        