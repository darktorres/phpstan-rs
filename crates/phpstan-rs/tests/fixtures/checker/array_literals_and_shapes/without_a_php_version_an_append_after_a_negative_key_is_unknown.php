<?php
use function PHPStan\Testing\assertType;
assertType("non-empty-array<int, 'a'|'b'>", [-5 => 'a', 'b']);
assertType("array{3: 'a', -9: 'c', 4: 'b'}", [3 => 'a', -9 => 'c', 'b']);
