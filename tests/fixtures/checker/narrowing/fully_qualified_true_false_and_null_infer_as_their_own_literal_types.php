<?php
            function needsTrue(true $x): void {}
            function needsFalse(false $x): void {}
            function needsNull(null $x): void {}
            function test(): void {
                needsTrue(\true);
                needsFalse(\false);
                needsNull(\null);
            }
        