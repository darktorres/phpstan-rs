<?php
            enum Status { case A; case B; case C; }
            function f(Status $s): string {
                return match ($s) {
                    Status::A, Status::B, Status::C => 'covers everything',
                };
            }
        