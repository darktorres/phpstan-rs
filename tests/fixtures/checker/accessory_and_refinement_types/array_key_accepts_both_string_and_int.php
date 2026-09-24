<?php
            /** @param array-key $key */
            function f($key): void {}
            function test(): void { f("hello"); f(42); }
        