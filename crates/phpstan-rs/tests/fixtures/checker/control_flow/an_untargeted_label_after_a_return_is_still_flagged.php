<?php
            function f(): int {
                return 1;
                never_reached:
                echo "dead";
            }
        