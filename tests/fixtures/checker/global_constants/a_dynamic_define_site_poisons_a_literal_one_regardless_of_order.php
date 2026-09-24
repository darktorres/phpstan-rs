<?php
            class Config {
                public int $verbosity = 0;
            }
            function withDynamicValue(Config $c): void { define('FOO', $c->verbosity); }
            function withLiteralValue(): void { define('FOO', 0); }
            function needsString(string $x): void {}
            needsString(FOO);
        