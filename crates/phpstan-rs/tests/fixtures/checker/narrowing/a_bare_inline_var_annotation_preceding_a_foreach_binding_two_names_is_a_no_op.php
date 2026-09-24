<?php
            class FakeFileInfo9921 {}
            function f(array $items): string {
                /** @var FakeFileInfo9921 */
                foreach ($items as $k => $v) {
                    return $v;
                }
                return "fallback";
            }
        