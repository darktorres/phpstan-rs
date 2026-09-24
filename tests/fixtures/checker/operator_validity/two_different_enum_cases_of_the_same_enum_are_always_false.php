<?php
            enum Status { case A; case B; }
            function f(): void {
                if (Status::A === Status::B) { echo 1; }
            }
        