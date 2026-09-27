<?php
function bu(array $a): int { $k = array_key_first($a); return $k === null ? 0 : strlen($k); }
