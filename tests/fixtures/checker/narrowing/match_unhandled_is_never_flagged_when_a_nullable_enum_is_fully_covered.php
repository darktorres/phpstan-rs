<?php
            enum Status { case Active; case Inactive; }
            function f(Status|null $s): string {
                return match ($s) {
                    Status::Active => "active",
                    Status::Inactive => "inactive",
                    null => "none",
                };
            }
        