<?php
            enum Status { case A; case B; }
            function f(Status $s) {
                if (is_object($s)) { echo 1; }
            }
        