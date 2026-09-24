<?php
            /** @param array-key $key */
            function f($key): void {}
            function test(): void { f([1, 2]); }
        