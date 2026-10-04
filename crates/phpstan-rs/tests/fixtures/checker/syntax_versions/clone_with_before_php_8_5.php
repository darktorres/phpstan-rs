<?php
$o = new stdClass();
$p = clone($o, ["a" => 1]);
$q = clone $o;
