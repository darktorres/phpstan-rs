<?php
        function f(): void {
            $attempts = 0;
            do {
                $attempts++;
            } while ($attempts < 150);
        }
        