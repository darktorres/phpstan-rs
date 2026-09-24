<?php
            /** @param int ...$nums */
            function sumDocVariadic(...$nums): void {}
            function test(): void { sumDocVariadic(1, 2, "not an int"); }
        