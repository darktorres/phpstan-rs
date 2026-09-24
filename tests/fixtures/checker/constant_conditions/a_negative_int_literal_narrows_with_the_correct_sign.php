<?php
        function f(int $id): void {
            if ($id < -32768 || $id > 32767) {
                return;
            }
        }
        