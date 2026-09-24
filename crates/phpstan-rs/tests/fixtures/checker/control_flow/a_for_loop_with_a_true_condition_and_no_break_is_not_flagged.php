<?php
            function f(): string {
                for ($i = 0; true; $i++) {
                    if ($i > 3) {
                        return 'x';
                    }
                }
            }
        