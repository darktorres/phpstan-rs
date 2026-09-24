<?php
            define('FOO', 42);
            function needsInt(int $x): void {}
            needsInt(FOO);
        