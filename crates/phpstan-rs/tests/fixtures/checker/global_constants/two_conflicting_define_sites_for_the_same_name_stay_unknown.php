<?php
            function firstSite(): void { define('FOO', 0); }
            function secondSite(): void { define('FOO', 'hello'); }
            function needsInt(int $x): void {}
            needsInt(FOO);
        