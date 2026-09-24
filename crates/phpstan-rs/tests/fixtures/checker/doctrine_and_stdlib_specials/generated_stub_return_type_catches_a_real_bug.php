<?php
            function needsString(string $s): string { return $s; }
            needsString(json_encode(["a" => 1]));
        