<?php
use function PHPStan\Testing\assertType;
assertType("array{-5: 'a', -4: 'b'}", [-5 => 'a', 'b']);
assertType("array{3: 'a', -9: 'c', 4: 'b'}", [3 => 'a', -9 => 'c', 'b']);
