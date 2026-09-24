<?php
            function f(): int {
                try {
                    return 1;
                } finally {
                    echo "cleanup";
                }
            }
        