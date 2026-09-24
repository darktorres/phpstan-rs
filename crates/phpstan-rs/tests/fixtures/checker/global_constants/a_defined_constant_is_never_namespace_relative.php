<?php
            namespace App;
            define('FOO', 'hello');
            function needsInt(int $x): void {}
            needsInt(FOO);
        