<?php
            class Legacy { public string $password = ""; }
            function f(Legacy $l): string { return $l->password; }
        