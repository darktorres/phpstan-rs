<?php
namespace App;

function helper(int $x): int { return $x + 1; }

function twice(int $x): int { return helper($x) * 2; }

twice(1);
