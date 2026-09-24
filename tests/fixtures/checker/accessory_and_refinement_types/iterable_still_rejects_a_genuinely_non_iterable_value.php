<?php
            function f(iterable $items): void {}
            function test(): void { f("not iterable"); }
        