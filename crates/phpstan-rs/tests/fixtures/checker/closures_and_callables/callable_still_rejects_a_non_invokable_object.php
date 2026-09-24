<?php
            class NotInvokable {}
            function apply(callable $fn): void {}
            function test(NotInvokable $n): void { apply($n); }
        