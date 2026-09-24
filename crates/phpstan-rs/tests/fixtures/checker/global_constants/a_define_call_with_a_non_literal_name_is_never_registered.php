<?php
            function name(): string { return 'FOO'; }
            define(name(), 42);
            function needsInt(int $x): void {}
            needsInt(FOO);
        