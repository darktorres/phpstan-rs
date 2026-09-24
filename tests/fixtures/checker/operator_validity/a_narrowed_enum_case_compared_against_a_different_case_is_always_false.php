<?php
            enum Status { case A; case B; }
            function f(Status $s): void {
                if ($s === Status::A) {
                    if ($s === Status::B) {
                        echo 1;
                    }
                }
            }
        