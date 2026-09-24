<?php
            function sideEffect(): bool { echo 'x'; return true; }
            function test(bool $a): void {
                $a && sideEffect();
            }
        