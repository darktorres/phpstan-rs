<?php
            class NoToString {}
            function display(string|Stringable $x): void {}
            function test(NoToString $n): void { display($n); }
        