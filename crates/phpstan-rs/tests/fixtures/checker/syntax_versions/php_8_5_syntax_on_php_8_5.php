<?php
$o = new stdClass();
$p = clone($o, ["a" => 1]);
echo "a" |> strlen(...);
